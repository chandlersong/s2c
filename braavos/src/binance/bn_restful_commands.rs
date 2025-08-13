use crate::binance::bn_models::{
    BINANCE_API_BASE, EXCHANGE_INFO_PATH, PING_PATH, SERVER_TIME_PATH, SecurityInfo,
};
use crate::errors::BraavosError;
use crate::http_client::HTTP_CLIENT;
use crate::models::{EmptyObject, RequestInfo};
use crate::tools::sign_hmac;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::fmt::Display;
use std::num::NonZeroU32;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::time::timeout;
use ureq::Request;

pub static PING_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_API_BASE, PING_PATH, false, 1).unwrap());

///币安当前有 1479 个交易对
/// 时区: UTC
/// 服务器时间: 1754383074378
/// 限频规则: [RateLimit { rate_limit_type: "REQUEST_WEIGHT", interval: "MINUTE", interval_num: 1, limit: 6000 }, RateLimit { rate_limit_type: "ORDERS", interval: "SECOND", interval_num: 10, limit: 100 }, RateLimit { rate_limit_type: "ORDERS", interval: "DAY", interval_num: 1, limit: 200000 }, RateLimit { rate_limit_type: "RAW_REQUESTS", interval: "MINUTE", interval_num: 5, limit: 61000 }]
/**
{
    symbol:                              "ETHBTC",
    status:                              "TRADING",
    base_asset:                          "ETH",
    base_asset_precision:                8,
    quote_asset:                         "BTC",
    quote_precision:                     8,
    quote_asset_precision:               8,
    base_commission_precision:           8,
    quote_commission_precision:          8,
    order_types:                         [
      "LIMIT",
      "LIMIT_MAKER",
      "MARKET",
      "STOP_LOSS",
      "STOP_LOSS_LIMIT",
      "TAKE_PROFIT",
      "TAKE_PROFIT_LIMIT"
    ],
    iceberg_allowed:                     true,
    oco_allowed:                         true,
    quote_order_qty_market_allowed:      true,
    allow_trailing_stop:                 true,
    cancel_replace_allowed:              true,
    is_spot_trading_allowed:             true,
    is_margin_trading_allowed:           true,
    filters:                             [
      PriceFilter
      {min_price: Some("0.00001000"), max_price: Some("922327.00000000"), tick_size: Some("0.00001000")},
      LotSize
      {min_qty: Some("0.00010000"), max_qty: Some("100000.00000000"), step_size: Some("0.00010000")},
      Unknown,
      Unknown,
      Unknown,
      Unknown,
      Unknown,
      Unknown,
      Unknown
    ],
    permissions:                         [],
    default_self_trade_prevention_mode:  "EXPIRE_MAKER",
    allowed_self_trade_prevention_modes: ["EXPIRE_TAKER", "EXPIRE_MAKER", "EXPIRE_BOTH", "DECREMENT"]
  }
**/
pub static EXCHANGE_INFO_COMMAND: LazyLock<RequestInfo> = LazyLock::new(|| {
    RequestInfo::from_base_path(BINANCE_API_BASE, EXCHANGE_INFO_PATH, false, 20).unwrap()
});

pub static SERVER_TIME_COMMAND: LazyLock<RequestInfo> = LazyLock::new(|| {
    RequestInfo::from_base_path(BINANCE_API_BASE, SERVER_TIME_PATH, false, 1).unwrap()
});

/// 全局 RateLimiter，使用 OnceLock 延迟初始化
static RATE_LIMITER: OnceLock<RateLimiter<NotKeyed, InMemoryState, DefaultClock>> = OnceLock::new();

/// 获取 RateLimiter 的静态引用
fn get_bn_rate_limiter(
    per_second_num: u32,
) -> &'static RateLimiter<NotKeyed, InMemoryState, DefaultClock> {
    //TODO：按照
    RATE_LIMITER.get_or_init(|| {
        RateLimiter::direct(
            Quota::per_second(NonZeroU32::new(per_second_num).unwrap())
                .allow_burst(NonZeroU32::new(per_second_num).unwrap()),
        )
    })
}

async fn check_rate_limit(weight: u32) -> Result<(), BraavosError> {
    let limiter = get_bn_rate_limiter(1200);
    // 超时时间：2 秒
    let timeout_duration = Duration::from_secs(2);
    // 抖动避免请求堆积
    let jitter = Jitter::up_to(Duration::from_millis(100));

    // 验证权重非零
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(BraavosError::new("权重必须为非零")),
    };
    // 等待令牌或超时
    let result = timeout(
        timeout_duration,
        limiter.until_n_ready_with_jitter(weight, jitter),
    )
    .await;
    match result {
        Ok(inner_result) => match inner_result {
            Ok(()) => Ok(()),
            Err(_) => Err(BraavosError::new("令牌不足")),
        },
        Err(_) => Err(BraavosError::new("限流超时")),
    }
}

pub async fn execute_ping() -> Result<(), BraavosError> {
    let _ = execute_bn_get::<EmptyObject, EmptyObject>(&PING_COMMAND, None, None).await?;
    Ok(())
}

pub async fn execute_bn_get<T: Display, U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<T>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let request = create_request_with_param_and_security(info, param, "GET", security_info)?;
    let res = request.call()?;
    let result: U = res.into_json()?;
    Ok(result)
}

pub async fn execute_bn_post<T: Display, U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<T>,
    body: Option<Value>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let request = create_request_with_param_and_security(info, param, "POST", security_info)?;
    let request_body = body.unwrap_or_else(|| Value::Null);
    let res = request.send_json(&request_body)?;
    let result: U = res.into_json()?;
    Ok(result)
}

pub async fn execute_bn_put<T: Display, U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<T>,
    body: Option<Value>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let request = create_request_with_param_and_security(info, param, "PUT", security_info)?;
    let request_body = match body {
        None => Value::Null,
        Some(body_json) => body_json,
    };
    let res = request.send_json(&request_body)?;
    let result: U = res.into_json()?;
    Ok(result)
}

fn create_request_with_param_and_security<T: Display>(
    info: &RequestInfo,
    param: Option<T>,
    method: &str,
    security_info: Option<SecurityInfo>,
) -> Result<Request, BraavosError> {
    let client = HTTP_CLIENT
        .get()
        .ok_or(BraavosError::new("客户端没有初始化"))?;
    let mut url = info.as_ref().clone();
    param.map(|request| {
        let query_param = format!("{}", request);
        let real_param = match &security_info {
            None => query_param,
            Some(info) => {
                let signature = sign_hmac(&query_param, &info.api_secret).unwrap();
                match query_param.is_empty() {
                    true => {
                        format!("signature={signature}")
                    }
                    false => {
                        format!("{query_param}&signature={signature}")
                    }
                }
            }
        };

        if !real_param.is_empty() {
            url.set_query(Some(&real_param));
        }
    });
    let request = client.request_url(method, &url);
    let request_with_security = match &security_info {
        None => request,
        Some(info) => request.set("X-MBX-APIKEY", &info.api_key),
    };
    Ok(request_with_security)
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_restful_commands::{check_rate_limit, get_bn_rate_limiter};

    #[tokio::test]
    async fn test_rate_limited() {
        get_bn_rate_limiter(1200);
        // 测试正常调用
        let result = check_rate_limit(1).await;
        assert!(result.is_ok());

        // 测试高权重调用，触发超时
        let result = check_rate_limit(1201).await; // 超过突发容量
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_zero_weight() {
        // 测试零权重，预期错误
        let result = check_rate_limit(0).await;
        assert!(result.is_err());
    }
}

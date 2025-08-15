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
use std::collections::BTreeMap;
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
    let _ = execute_bn_get::<EmptyObject>(&PING_COMMAND, None, None).await?;
    Ok(())
}

pub async fn execute_bn_get<U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<BTreeMap<&str, String>>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let request = create_request_with_param_and_security(info, param, "GET", security_info)?;
    let res = request.call()?;
    let result: U = res.into_json()?;
    Ok(result)
}

pub async fn execute_bn_post<U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<BTreeMap<&str, String>>,
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

pub async fn execute_bn_put<U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<BTreeMap<&str, String>>,
    body: Option<Value>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let request = create_request_with_param_and_security(info, param, "PUT", security_info)?;
    let request_body = body.unwrap_or_else(|| Value::Null);
    let res = request.send_json(&request_body)?;
    let result: U = res.into_json()?;
    Ok(result)
}

fn create_request_with_param_and_security(
    info: &RequestInfo,
    param: Option<BTreeMap<&str, String>>,
    method: &str,
    security_info: Option<SecurityInfo>,
) -> Result<Request, BraavosError> {
    let client = HTTP_CLIENT
        .get()
        .ok_or(BraavosError::new("客户端没有初始化"))?;
    let mut url = info.as_ref().clone();

    // 1. Build the base query string from params.
    let base_query_string = param
        .filter(|p| !p.is_empty()) // Treat None and empty map the same
        .map(|params| {
            params
                .into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<String>>()
                .join("&")
        })
        .unwrap_or_default(); // If None or empty, this is an empty String

    // 2. Handle security and signature
    if let Some(sec_info) = &security_info {
        // Always sign if security_info is present
        let signature = sign_hmac(&base_query_string, &sec_info.api_secret).unwrap();
        let final_query = if base_query_string.is_empty() {
            format!("signature={}", signature)
        } else {
            format!("{}&signature={}", base_query_string, signature)
        };
        url.set_query(Some(&final_query));
    } else if !base_query_string.is_empty() {
        // No security, but there are params
        url.set_query(Some(&base_query_string));
    }

    // 3. Create request and set header
    let request = client.request_url(method, &url);
    let request_with_security = match &security_info {
        None => request,
        Some(info) => request.set("X-MBX-APIKEY", &info.api_key),
    };

    Ok(request_with_security)
}

#[cfg(test)]
mod tests {
    use super::{
        HTTP_CLIENT, check_rate_limit, create_request_with_param_and_security, get_bn_rate_limiter,
    };
    use crate::binance::bn_models::{BINANCE_API_BASE, SecurityInfo};
    use crate::models::RequestInfo;
    use crate::tools::sign_hmac;
    use std::collections::BTreeMap;
    use ureq::Agent;

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

    #[test]
    fn test_create_request_with_param_and_security() {
        // Initialize HTTP_CLIENT for testing
        HTTP_CLIENT.set(Agent::new()).unwrap();

        let request_info =
            RequestInfo::from_base_path(BINANCE_API_BASE, "/api/v3/order", false, 1).unwrap();
        let security_info = SecurityInfo {
            api_key: "test_api_key".to_string(),
            api_secret: "test_api_secret".to_string(),
        };

        // Scenario 1: No params, no security
        let request =
            create_request_with_param_and_security(&request_info, None, "GET", None).unwrap();
        assert_eq!(
            request.url(),
            "https://api.binance.com/api/v3/order",
            "Scenario 1 (No params, no security): URL should be base path"
        );
        assert_eq!(
            request.header("X-MBX-APIKEY"),
            None,
            "Scenario 1 (No params, no security): API key header should not be set"
        );

        // Scenario 2: Params, no security
        let mut params_map = BTreeMap::new();
        params_map.insert("symbol", "BTCUSDT".to_string());
        params_map.insert("side", "BUY".to_string());
        params_map.insert("type", "LIMIT".to_string()); // BTreeMap will sort this
        let request = create_request_with_param_and_security(
            &request_info,
            Some(params_map.clone()),
            "POST",
            None,
        )
        .unwrap();
        assert_eq!(
            request.url(),
            "https://api.binance.com/api/v3/order?side=BUY&symbol=BTCUSDT&type=LIMIT", // Note the alphabetical order
            "Scenario 2 (Params, no security): URL should include sorted query parameters"
        );
        assert_eq!(
            request.header("X-MBX-APIKEY"),
            None,
            "Scenario 2 (Params, no security): API key header should not be set"
        );

        // Scenario 3: No params, security
        let request = create_request_with_param_and_security(
            &request_info,
            None,
            "GET",
            Some(security_info.clone()),
        )
        .unwrap();
        let signature_for_empty = sign_hmac("", "test_api_secret").unwrap();
        let expected_url_3 = format!(
            "https://api.binance.com/api/v3/order?signature={}",
            signature_for_empty
        );
        assert_eq!(
            request.url(),
            expected_url_3,
            "Scenario 3 (No params, security): URL should contain only the signature"
        );
        assert_eq!(
            request.header("X-MBX-APIKEY"),
            Some("test_api_key"),
            "Scenario 3 (No params, security): API key header should be set"
        );

        // Scenario 4: Empty params, security
        let empty_map = BTreeMap::new();
        let request = create_request_with_param_and_security(
            &request_info,
            Some(empty_map),
            "POST",
            Some(security_info.clone()),
        )
        .unwrap();
        // The signature and URL should be identical to scenario 3
        assert_eq!(
            request.url(),
            expected_url_3, // Use the same expected URL from scenario 3
            "Scenario 4 (Empty params, security): URL should contain only the signature"
        );
        assert_eq!(
            request.header("X-MBX-APIKEY"),
            Some("test_api_key"),
            "Scenario 4 (Empty params, security): API key header should be set"
        );

        // Scenario 5: Params, security
        let request = create_request_with_param_and_security(
            &request_info,
            Some(params_map), // Use the map from scenario 2
            "POST",
            Some(security_info.clone()),
        )
        .unwrap();
        let query_string = "side=BUY&symbol=BTCUSDT&type=LIMIT";
        let signature = sign_hmac(query_string, "test_api_secret").unwrap();
        let expected_url_5 = format!(
            "https://api.binance.com/api/v3/order?{}&signature={}",
            query_string, signature
        );
        assert_eq!(
            request.url(),
            expected_url_5,
            "Scenario 5 (Params, security): URL should contain sorted params and signature"
        );
        assert_eq!(
            request.header("X-MBX-APIKEY"),
            Some("test_api_key"),
            "Scenario 5 (Params, security): API key header should be set"
        );
    }
}

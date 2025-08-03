use crate::binance::bn_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, SecurityInfo};
use crate::errors::BraavosError;
use crate::http_client::HTTP_CLIENT;
use crate::models::EmptyObject;
use crate::tools::sign_hmac;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use log::{error, trace};
use reqwest::{RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde_json::{Error as JsonError, Value};
use std::fmt::Display;
use std::num::NonZeroU32;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::time::timeout;


static PING_COMMAND: LazyLock<CommandInfo> = LazyLock::new(|| {
    CommandInfo {
        base: BinanceBase::Normal,
        path: BinancePath::Normal(NormalAPI::PingAPI),
        has_security: false,
        weight: 1,
    }
});

/// 全局 RateLimiter，使用 OnceLock 延迟初始化
static RATE_LIMITER: OnceLock<RateLimiter<NotKeyed, InMemoryState, DefaultClock>> = OnceLock::new();

/// 获取 RateLimiter 的静态引用
fn get_bn_rate_limiter(per_second_num: u32) -> &'static RateLimiter<NotKeyed, InMemoryState, DefaultClock> {
    RATE_LIMITER.get_or_init(|| RateLimiter::direct(
        Quota::per_second(NonZeroU32::new(per_second_num).unwrap())
            .allow_burst(NonZeroU32::new(per_second_num).unwrap())
    ))

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
        None => return Err(BraavosError::new("权重必须为非零"))
    };
    // 等待令牌或超时
    let result = timeout(
        timeout_duration,
        limiter.until_n_ready_with_jitter(weight, jitter),
    ).await;
    match result {
        Ok(inner_result) => match inner_result {
            Ok(()) => Ok(()),
            Err(_) => Err(BraavosError::new("令牌不足"))
        },
        Err(_) => Err(BraavosError::new("限流超时")),
    }
}

pub async fn execute_ping() -> Result<(), BraavosError> {
    let _ = execute_bn_get::<EmptyObject, EmptyObject>(&PING_COMMAND, None, None).await?;
    Ok(())
}

pub async fn execute_bn_get<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>, security_info: Option<SecurityInfo>) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request = create_request_with_param_and_security(info, param, |url| client.get(url), security_info)?;
    let res = request.send().await?;
    trace!("Response: {:?} {}", res.version(), res.status());
    let body = res.text().await?;
    trace!("body:{}",&body);
    let result: Result<U, JsonError> = serde_json::from_str(&body);
    match result {
        Ok(resp1) => Ok(resp1),
        Err(_) => {
            error!("binance error response,{}",&body);
            Err(BraavosError::new(&body))
        }
    }
}

pub async fn execute_bn_post<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>, body: Option<Value>, security_info: Option<SecurityInfo>) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request_with_security = create_request_with_param_and_security(info, param, |url| client.post(url), security_info)?;
    let request_with_body = match body {
        None => {
            request_with_security
        }
        Some(body_json) => {
            request_with_security.json(&body_json)
        }
    };
    let res = request_with_body.send().await?;
    trace!("Response: {:?} {}", res.version(), res.status());
    let body = res.text().await?;
    trace!("body:{}",&body);
    let result: Result<U, JsonError> = serde_json::from_str(&body);
    match result {
        Ok(resp1) => Ok(resp1),
        Err(_) => {
            error!("binance error response,{}",&body);
            Err(BraavosError::new(&body))
        }
    }
}


pub async fn execute_bn_put<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>, body: Option<Value>, security_info: Option<SecurityInfo>) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request_with_security = create_request_with_param_and_security(info, param, |url| client.put(url), security_info)?;
    let request_with_body = match body {
        None => {
            request_with_security
        }
        Some(body_json) => {
            request_with_security.json(&body_json)
        }
    };
    let res = request_with_body.send().await?;
    trace!("Response: {:?} {}", res.version(), res.status());
    let body = res.text().await?;
    trace!("body:{}",&body);
    let result: Result<U, JsonError> = serde_json::from_str(&body);
    match result {
        Ok(resp1) => Ok(resp1),
        Err(_) => {
            error!("binance error response,{}",&body);
            Err(BraavosError::new(&body))
        }
    }
}

fn create_request_with_param_and_security<T: Display, F>(info: &CommandInfo, param: Option<T>, method: F, security_info: Option<SecurityInfo>) -> Result<RequestBuilder, BraavosError>
where
    F: Fn(Url) -> RequestBuilder,
{
    let mut url = Url::parse(&String::from(info.base.clone())).expect("Invalid base URL");
    url.set_path(&String::from(&String::from(info.path.clone())));
    param.map(|request| {
        let query_param = format!("{}", request);
        let real_param = match &security_info {
            None => { query_param }
            Some(info) => {
                let signature = sign_hmac(&query_param, &info.api_secret).unwrap();
                format!("{query_param}&signature={signature}")
            }
        };

        url.set_query(Some(&real_param));
    });
    let request_builder = method(url);
    let request_with_security = match &security_info {
        None => {
            request_builder
        }
        Some(info) => {
            request_builder.header(
                "X-MBX-APIKEY", &info.api_key,
            )
        }
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

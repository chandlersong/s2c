use crate::binance::bn_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, SecurityInfo};
use crate::errors::BraavosError;
use crate::http_client::HTTP_CLIENT;
use crate::models::EmptyObject;
use crate::tools::sign_hmac;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use log::{error, trace};
use nonzero_ext::nonzero;
use reqwest::{RequestBuilder, Url};
use serde::de::DeserializeOwned;
use serde_json::{Error as JsonError, Value};
use std::fmt::Display;
use std::sync::{LazyLock, OnceLock};

pub(crate) static BN_SECURITY: OnceLock<SecurityInfo> = OnceLock::new();

pub fn init_bn_security(api_key: String, api_secret: String) -> &'static SecurityInfo {
    BN_SECURITY.get_or_init(|| SecurityInfo {
        api_key,
        api_secret,
    })
}
/// 全局 RateLimiter，使用 OnceLock 延迟初始化
static RATE_LIMITER: LazyLock<RateLimiter<NotKeyed, InMemoryState, DefaultClock>> =  LazyLock::new(|| {
    get_rate_limiter()
});



/// 获取 RateLimiter 的静态引用
fn get_rate_limiter() -> RateLimiter<NotKeyed, InMemoryState, DefaultClock> {
        RateLimiter::direct(
            Quota::per_second(nonzero!(10u32)) // 每秒补充 10 个令牌
                .allow_burst(nonzero!(20u32)) // 突发容量 20 个令牌
        )
}


static PING_COMMAND: LazyLock<CommandInfo> = LazyLock::new(|| {
    CommandInfo {
        base: BinanceBase::Normal,
        path: BinancePath::Normal(NormalAPI::PingAPI),
        has_security: false,
        weight: 0,
    }
});





pub async fn execute_ping() -> Result<(), BraavosError> {
    let _ = execute_bn_get::<EmptyObject, EmptyObject>(&PING_COMMAND, None).await?;
    Ok(())
}

pub async fn execute_bn_get<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>) -> Result<U, BraavosError> {
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request = create_request_with_param_and_security(info, param, |url| client.get(url))?;
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

pub async fn execute_bn_post<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>, body: Option<Value>) -> Result<U, BraavosError> {
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request_with_security = create_request_with_param_and_security(info, param, |url| client.post(url))?;
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


pub async fn execute_bn_put<T: Display, U: DeserializeOwned>(info: &CommandInfo, param: Option<T>, body: Option<Value>) -> Result<U, BraavosError> {
    let client = HTTP_CLIENT.get().ok_or(BraavosError::new("客户端没有初始化"))?;
    let request_with_security = create_request_with_param_and_security(info, param, |url| client.put(url))?;
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

fn create_request_with_param_and_security<T: Display, F>(info: &CommandInfo, param: Option<T>, method: F) -> Result<RequestBuilder, BraavosError>
where
    F: Fn(Url) -> RequestBuilder,
{
    let mut url = Url::parse(&String::from(info.base.clone())).expect("Invalid base URL");
    url.set_path(&String::from(&String::from(info.path.clone())));
    let security = BN_SECURITY.get().ok_or(BraavosError::new("没有配置用户信息"))?;
    param.map(|request| {
        let query_param = format!("{}", request);
        let real_param = match info.has_security {
            false => { query_param }
            true => {
                let signature = sign_hmac(&query_param, &security.api_secret).unwrap();
                format!("{query_param}&signature={signature}")
            }
        };

        url.set_query(Some(&real_param));
    });
    let request_builder = method(url);
    let request_with_security = match info.has_security {
        false => {
            request_builder
        }
        true => {
            request_builder.header(
                "X-MBX-APIKEY", &security.api_key,
            )
        }
    };
    Ok(request_with_security)
}




#[cfg(test)]
mod tests {



}

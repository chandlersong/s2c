use crate::errors::YueError;
use crate::models::RequestInfo;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, RateLimiter};
use reqwest::{Client, Method, RequestBuilder};
use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::timeout;

pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

pub type DefaultRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

pub fn init_http_client(proxy: Option<&str>) -> &'static Client {
    //TODO：像超时这类进行配置。

    HTTP_CLIENT.get_or_init(|| {
        let mut res = Client::builder();
        if let Some(proxy_url) = proxy {
            res = res.proxy(reqwest::Proxy::all(proxy_url).unwrap());
        } else {
            res = res.no_proxy(); // 明确禁用所有代理,否则他可能走系统代理
        }
        res.build().unwrap()
    })
}

pub trait YueRequestBuilder: Send + Sync {
    fn compose_request(
        &self,
        client: &Client,
        info: &RequestInfo,
        param: Option<String>,
        method: Method,
    ) -> Result<RequestBuilder, YueError>;
}

pub async fn check_rate_limit(weight: u32, limiter: &DefaultRateLimiter) -> Result<(), YueError> {
    // 超时时间：2 秒
    let timeout_duration = Duration::from_secs(2);
    // 抖动避免请求堆积
    let jitter = Jitter::up_to(Duration::from_millis(100));

    // 验证权重非零
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(YueError::new("权重必须为非零")),
    };
    // 等待令牌或�����时
    let result = timeout(
        timeout_duration,
        limiter.until_n_ready_with_jitter(weight, jitter),
    )
    .await;
    match result {
        Ok(inner_result) => match inner_result {
            Ok(()) => Ok(()),
            Err(_) => Err(YueError::new("令牌不足")),
        },
        Err(_) => Err(YueError::new("限流超时")),
    }
}

#[derive(Clone)]
pub struct NonAuthRequestBuilder {}

impl NonAuthRequestBuilder {
    pub fn new() -> Self {
        NonAuthRequestBuilder {}
    }
}

impl YueRequestBuilder for NonAuthRequestBuilder {
    fn compose_request(
        &self,
        client: &Client,
        info: &RequestInfo,
        param: Option<String>,
        method: Method,
    ) -> Result<RequestBuilder, YueError> {
        let mut url = info.as_ref().clone();
        if let Some(p) = param {
            if !p.is_empty() {
                url.set_query(Some(&p));
            }
        };
        let request = client.request(method, url.to_string());
        Ok(request)
    }
}

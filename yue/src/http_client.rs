use crate::errors::YueError;
use crate::models::RequestInfo;
use backon::{Backoff, Retryable};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, RateLimiter};
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::future::Future;
use std::marker::PhantomData;
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
    fn compose_request(&self, client: &Client, info: &RequestInfo, param: Option<String>, method: Method) -> Result<RequestBuilder, YueError>;
}

pub async fn check_rate_limit(weight: u32, limiter: &DefaultRateLimiter) -> Result<(), YueError> {
    // 超时时间：2 秒
    let timeout_duration = Duration::from_secs(60);
    // 抖动避免请求堆积
    let jitter = Jitter::up_to(Duration::from_millis(100));

    // 验证权重非零
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(YueError::new("权重必须为非零")),
    };
    // 等待令牌或�����时
    let result = timeout(timeout_duration, limiter.until_n_ready_with_jitter(weight, jitter)).await;
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
    fn compose_request(&self, client: &Client, info: &RequestInfo, param: Option<String>, method: Method) -> Result<RequestBuilder, YueError> {
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

macro_rules! check_status {
    ($res:expr) => {
        if $res.status() != reqwest::StatusCode::OK {
            return Err(YueError::ExchangeRequestError {
                code: $res.status().as_u16(),
                body: $res.text().await.unwrap_or_default(),
            });
        }
    };
}

/// 通用请求包装器，支持限流和重试
pub struct YueRequest<'a, T, U>
where
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned,
{
    pub info: &'a RequestInfo,
    pub param: Option<String>,
    pub request_builder: T,
    pub body: Option<&'a Value>,
    pub method: Method,
    pub _phantom: PhantomData<U>,
}

impl<'a, T, U> YueRequest<'a, T, U>
where
    T: YueRequestBuilder + Clone + 'a,
    U: DeserializeOwned,
{
    async fn perform_request_async(
        info: &'a RequestInfo,
        param: Option<String>,
        request_builder: &T,
        body: Option<&'a Value>,
        method: Method,
        rate_limit: Option<&'a DefaultRateLimiter>,
    ) -> Result<U, YueError> {
        if let Some(limiter) = rate_limit {
            check_rate_limit(info.weight, limiter).await?;
        }
        let client = HTTP_CLIENT.get().ok_or(YueError::new("客户端没有初始化"))?;
        let mut request = request_builder.compose_request(client, info, param, method.clone())?;
        if method == Method::POST || method == Method::PUT {
            if let Some(body) = body {
                request = request.json(body);
            }
        }
        let res = request.send().await?;
        check_status!(res);
        let result: U = res.json::<U>().await?;
        Ok(result)
    }

    pub async fn execute(&self, rate_limit: Option<&'a DefaultRateLimiter>) -> Result<U, YueError> {
        Self::perform_request_async(
            self.info,
            self.param.clone(),
            &self.request_builder,
            self.body,
            self.method.clone(),
            rate_limit,
        )
        .await
    }

    pub fn into_retryable(
        self,
        rate_limit: Option<&'a DefaultRateLimiter>,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<U, YueError>> + Send + 'a>> + 'a {
        let info = self.info;
        let param = self.param.clone();
        let request_builder = self.request_builder;
        let body = self.body;
        let method = self.method;
        move || {
            let info = info;
            let param = param.clone();
            let request_builder = request_builder.clone();
            let body = body;
            let method = method.clone();
            Box::pin(async move { YueRequest::<T, U>::perform_request_async(info, param, &request_builder, body, method.clone(), rate_limit).await })
        }
    }

    pub fn retry<B: Backoff>(self, builder: B, rate_limit: Option<&'a DefaultRateLimiter>) -> impl Future<Output = Result<U, YueError>> {
        self.into_retryable(rate_limit).retry(builder)
    }
}

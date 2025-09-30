use crate::errors::YueError;
use crate::models::RequestInfo;
use async_trait::async_trait;
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

pub async fn check_rate_limit(weight: u32, limiter: &DefaultRateLimiter, timeout_secs: u64) -> Result<(), YueError> {
    // 超时时间：2 秒
    let timeout_duration = Duration::from_secs(timeout_secs);
    // 抖动避免请求堆积
    let jitter = Jitter::up_to(Duration::from_millis(100));

    // 验证权重非零
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(YueError::new("权重必须为非零")),
    };
    // 等待令牌等待是
    let result = timeout(timeout_duration, limiter.until_n_ready_with_jitter(weight, jitter)).await;
    match result {
        Ok(inner_result) => match inner_result {
            Ok(()) => Ok(()),
            Err(_) => Err(YueError::new("令牌不足")),
        },
        Err(_) => Err(YueError::new("限流超时")),
    }
}

/// Trait：用于抽象 HTTP 响应处理逻辑，每个交易所可自定义实现
#[async_trait]
pub trait ResponseHandler<U>: Send + Sync + Clone {
    async fn handle_response(&self, res: reqwest::Response) -> Result<U, YueError>;
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

/// 通用请求包装器，支持限流、重试和自定义响应处理
pub struct YueRequest<'a, T, U, H>
where
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned,
    H: ResponseHandler<U>,
{
    pub info: &'a RequestInfo,
    pub param: Option<String>,
    pub request_builder: T,
    pub body: Option<&'a Value>,
    pub method: Method,
    pub response_handler: H, // 新增属性
    pub _phantom: PhantomData<U>,
}

impl<'a, T, U, H> YueRequest<'a, T, U, H>
where
    T: YueRequestBuilder + Clone + 'a,
    U: DeserializeOwned + Send + Sync + 'static,
    H: ResponseHandler<U> + 'a,
{
    async fn perform_request_async(&self) -> Result<U, YueError> {
        if let Some(limiter) = self.info.rate_limit {
            check_rate_limit(self.info.weight, limiter, self.info.get_timeout()).await?;
        }
        let client = HTTP_CLIENT.get().ok_or(YueError::new("客户端没有初始化"))?;
        let mut request = self
            .request_builder
            .compose_request(client, self.info, self.param.clone(), self.method.clone())?;
        if self.method == Method::POST || self.method == Method::PUT {
            if let Some(body) = self.body {
                request = request.json(body);
            }
        }
        let res = request.send().await?;
        // 调用 trait 处理响应
        self.response_handler.handle_response(res).await
    }

    pub async fn execute(&self) -> Result<U, YueError> {
        self.perform_request_async().await
    }

    pub fn into_retryable(self) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<U, YueError>> + Send + 'a>> + 'a {
        let info = self.info;
        let param = self.param.clone();
        let request_builder = self.request_builder;
        let body = self.body;
        let method = self.method;
        let response_handler = self.response_handler.clone();
        move || {
            let info = info;
            let param = param.clone();
            let request_builder = request_builder.clone();
            let body = body;
            let method = method.clone();
            let response_handler = response_handler.clone();
            Box::pin(async move {
                let req = YueRequest {
                    info,
                    param,
                    request_builder,
                    body,
                    method,
                    response_handler,
                    _phantom: PhantomData,
                };
                req.perform_request_async().await
            })
        }
    }

    pub fn retry<B: Backoff>(self, builder: B) -> impl Future<Output = Result<U, YueError>> {
        self.into_retryable().retry(builder)
    }
}

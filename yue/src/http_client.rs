use crate::errors::YueError;
use crate::models::{DefaultRateLimiter, RequestInfo};
use async_trait::async_trait;
use backon::{Backoff, Retryable};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, RateLimiter};
use log::{debug, error};
use reqwest::{Client, Method, RequestBuilder, StatusCode, header::HeaderMap};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::future::Future;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::time::Duration;

pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

pub fn init_http_client(proxy: Option<&str>) -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        let mut res = Client::builder();
        if let Some(proxy_url) = proxy {
            res = res.proxy(reqwest::Proxy::all(proxy_url).unwrap());
        } else {
            res = res.no_proxy(); // ��确禁用所有代理,否则他可能走系统代理
        }
        res.build().unwrap()
    })
}

pub trait YueRequestBuilder: Send + Sync {
    fn compose_request(&self, client: &Client, info: &RequestInfo, param: Option<String>, method: Method) -> Result<RequestBuilder, YueError>;
}

pub async fn check_rate_limit(weight: u32, limiter: &DefaultRateLimiter, timeout_secs: u64) -> Result<(), YueError> {
    // 超时时间：timeout_secs 秒
    let timeout_duration = Duration::from_secs(timeout_secs);
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(YueError::new("权重必须为非零")),
    };
    let jitter = Jitter::up_to(Duration::from_millis(500));
    // 优雅处理超时和 governor 错误
    match tokio::time::timeout(timeout_duration, limiter.until_n_ready_with_jitter(weight, jitter)).await {
        Err(e) => {
            error!("获取令牌超时, timeout 时间:{}秒, 错误:{}", timeout_secs, e);
            Err(YueError::new("限流超时"))
        }
        Ok(res) => match res {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("限流器内部错误: {:?}", e);
                Err(YueError::new(&format!("限流器内部错误: {:?}", e)))
            }
        },
    }
}

#[derive(Clone)]
pub struct ClonableResponseCache {
    pub body: Vec<u8>,
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl ClonableResponseCache {
    pub async fn from_response(res: reqwest::Response) -> Self {
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.bytes().await.unwrap_or_default().to_vec();
        ClonableResponseCache { body, status, headers }
    }
}

/// Trait：用于抽象 HTTP 响应处理逻辑，每个交易所可自定义实现
#[async_trait]
pub trait ResponseHandler<U>: Send + Sync + Clone {
    async fn handle_response(&self, resp: ClonableResponseCache, rate_limiter: Option<&DefaultRateLimiter>) -> Result<U, YueError>;
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

/// 通用请求包装器，支持限流、重试和自定��响应处理
#[deprecated]
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
            check_rate_limit(self.info.weight, limiter, self.info.get_rate_limit_timeout()).await?;
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
        debug!("execute request: {:?}", request);
        let res = request.send().await?;
        let resp_cache = ClonableResponseCache::from_response(res).await;
        let result = self.response_handler.handle_response(resp_cache.clone(), self.info.rate_limit).await;
        match result {
            Ok(val) => Ok(val),
            Err(e) => {
                let body_str = String::from_utf8_lossy(&resp_cache.body).to_string();
                error!("HTTP response error, status: {}, body: {}", resp_cache.status, body_str);
                Err(e)
            }
        }
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

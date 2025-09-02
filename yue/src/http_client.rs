use crate::errors::YueError;
use crate::models::RequestInfo;
use reqwest::{Client, Method, RequestBuilder};
use std::sync::OnceLock;

pub(crate) static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

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

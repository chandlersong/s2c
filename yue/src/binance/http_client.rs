use crate::errors::YueError;
use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
use governor::{
    Jitter, Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use log::error;
use rand;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{interval, sleep};

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BinanceSecurityType {
    #[serde(rename = "HMAC")]
    HMAC,
    #[serde(rename = "Ed25519")]
    Ed25519,
}

pub struct BinanceSecurityInfo {
    //FUTURE: 用security的那个包来包裹一下，优先级低
    /// security_type是Hmac则api_security是字符串
    /// security_type是Ed25519,则api_security是一个私钥的本地地址。
    api_key: String,
    api_secret: String,
    security_type: BinanceSecurityType,
}

#[derive(Clone)]
pub struct BinanceRestfulClient {
    client: Arc<Client>,
    max_retries: u16,
}

impl BinanceRestfulClient {
    /// 创建 limiter，立即从 exchangeInfo 获取限额并启动后台刷新任务
    pub async fn new(client: Arc<Client>) -> Arc<Self> {
        Arc::new(Self { client, max_retries: 5 })
    }

    ///
    /// # 币安的http调用接口。对于币安的规则。
    ///
    ///  整个流程。
    ///  1. 根据request_info中的host信息，获取令牌。如果超时，则报错。
    ///  2. 判断是否要加上权限，如果有的就加上签名
    ///  3. 发送请求。
    ///  4. 判断是否超时。
    ///
    ///  需要注意的点：
    ///  1. 如果请求http request和获取令牌的话，错误就重试，超过重试，再抛出error
    ///  2. 其他的错误，直接发出。
    ///
    /// # 具体功能
    /// ## 访问限制
    /// 1. 如果返回的http status code为429，则表示调用过多，需要降低访问频率。
    /// 2. 如果返回的http status code为418，则表示被封。
    /// 3. 如果发生访问限制，调用request_info的host中block_all_request。阻止其他的线程继续查询。等到回复后调用allow_all_request放行。
    /// 4. X-MBX-USED-WEIGHT是http的header。用来表示已经调用权重
    ///
    /// ### 429处理逻辑
    /// 1. 等待随机的ms数，第一次等待100到200的随机毫秒，如果还是429，那么加100ms的随机秒数。
    /// 2. 如果超过timeout。则报错。把表头的Retry-After写入到错误信息中抛出。
    ///
    /// ### 418处理逻辑
    /// 1. 报错。把表头的Retry-After写入到错误信息中抛出。
    ///
    /// ### X-MBX-USED-WEIGHT表头的处理。
    /// 1.可能有表头会显示如下。取值优先值按照其先后顺序。
    ///  - x-mbx-used-weight-1m
    ///  - x-mbx-used-weight-5m
    ///  - x-mbx-used-weight
    /// 2. 如果取到的值大于request_info的host的max_limit的少50，则等待100到300的随机值
    ///
    /// ## 接口鉴权
    /// 1. 根据request_info中的has_security来判断是否要启用加密。
    /// 2. 如果request_info为true，security为None，则报错。
    /// 3. 如果security的security_type为HMAC，则使用tools中的sign_hmac方法处理
    /// 4. 如果security的security_type为Ed25519，则使用tools中的sign_ed25519方法处理，通过load_ed25519_signing_key来加载私钥地址
    /// 5，把apiKey放在请求的header中的是X-MBX-APIKEY
    /// 6. 在queryParam里面加入一个新的signature=签名
    ///
    /// ### 签名算法
    /// 1. 将参数格式化为 参数=取值 对并用 & 分隔每个参数对。
    /// 2. 对字符串进行百分比编码（percent-encoded）。
    /// #### 计算payload
    /// query param是symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2
    /// 1. 加上必要的字段timestamp和recvWindow,recvWindow用
    ///    例子：symbol=１２３４５６=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
    /// 2. 对字符串进行百分比编码（percent-encoded）后，签名 payload 如下所示：
    ///    例子：symbol=%EF%BC%91%EF%BC%92%EF%BC%93%EF%BC%94%EF%BC%95%EF%BC%96&side=SELL&type=LIMIT&timeInForce=GTC&quantity=1&price=0.2&timestamp=1668481559918&recvWindow=5000
    /// 3. 根据相应的security_type的，调用相应的想法去加密
    ///
    /// 参考资料
    /// - [接口鉴权](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/rest-api/request-security)
    /// - [访问限制](https://developers.binance.com/docs/zh-CN/binance-spot-api-docs/rest-api/limits)
    ///
    pub async fn request(
        &self,
        builder: RequestBuilder,
        request_info: &RequestInfo,
        security: Option<BinanceSecurityInfo>,
    ) -> Result<Response, YueError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::BinanceRestfulClient;
    use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo};
    use governor::{Quota, RateLimiter};
    use reqwest::Client as ReqwestClient;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_mock_host_info(host: &str) -> Arc<HostInfo> {
        // 初始 quota（用一个合理默认值，马上会被刷新覆盖）
        let initial_quota = Quota::per_minute(NonZeroU32::new(1000).unwrap()).allow_burst(NonZeroU32::new(300).unwrap());

        let limiter = Arc::new(RwLock::new(Arc::new(DefaultRateLimiter::direct(initial_quota))));
        Arc::new(HostInfo::new(host, 0, limiter))
    }

    #[tokio::test]
    async fn minimal_mock() {
        let mock_server = wiremock::MockServer::start().await;

        Mock::given(wiremock::matchers::any())
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("OK"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let url = format!("{}/test", mock_server.uri());
        println!("Testing URL: {}", url);

        let resp = client.get(&url).send().await.unwrap();
        println!("Status: {}", resp.status());
        assert_eq!(resp.status().as_u16(), 200);
    }
}

use crate::binance::bn_models::{
    BINANCE_API_BASE, EXCHANGE_INFO_PATH, EmptyQueryParams, PING_PATH, SERVER_TIME_PATH,
    SPOT_KLINE_PATH, ToQueryParams,
};
use crate::errors::YueError;
use crate::errors::YueError::RequestError;
use crate::http_client::{HTTP_CLIENT, NonAuthRequestBuilder, YueRequestBuilder};
use crate::models::{EmptyObject, RequestInfo};
use crate::tools::sign_hmac;
use backon::{ExponentialBuilder, Retryable};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::time::timeout;

macro_rules! check_status {
    ($res:expr) => {
        if $res.status() != reqwest::StatusCode::OK {
            return Err(RequestError {
                code: $res.status().as_u16(),
                body: $res.text().await.unwrap_or_default(),
            });
        }
    };
}

#[derive(Clone)]
pub struct BNSecurityRequestBuilder {
    pub api_key: String,
    pub api_secret: String,
}

impl YueRequestBuilder for BNSecurityRequestBuilder {
    fn compose_request(
        &self,
        client: &Client,
        info: &RequestInfo,
        param: Option<String>,
        method: Method,
    ) -> Result<RequestBuilder, YueError> {
        let url = build_request_components(info, param, &self.api_secret);
        let mut request = client.request(method, &url);
        request = request.header("X-MBX-APIKEY", self.api_key.clone());
        Ok(request)
    }
}

fn build_request_components(info: &RequestInfo, param: Option<String>, api_secret: &str) -> String {
    let mut url = info.as_ref().clone();
    let base_query_string = param.filter(|s| !s.is_empty()).unwrap_or_default();
    let signature = sign_hmac(&base_query_string, &api_secret).unwrap();
    let final_query = if base_query_string.is_empty() {
        format!("signature={}", signature)
    } else {
        format!("{}&signature={}", base_query_string, signature)
    };
    url.set_query(Some(&final_query));

    url.to_string()
}

/// Wrapper for Binance requests to enable retry with backon
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
    _phantom: std::marker::PhantomData<U>,
}

impl<'a, T, U> YueRequest<'a, T, U>
where
    T: YueRequestBuilder + Clone + 'a,
    U: DeserializeOwned,
{
    pub async fn execute(&self) -> Result<U, YueError> {
        check_rate_limit(self.info.weight).await?;
        let client = HTTP_CLIENT.get().ok_or(YueError::new("客户端没有初始化"))?;
        let mut request = self.request_builder.compose_request(
            client,
            self.info,
            self.param.clone(),
            self.method.clone(),
        )?;
        if self.method == Method::POST || self.method == Method::PUT {
            if let Some(body) = self.body {
                request = request.json(body);
            }
        }
        let res = request.send().await?;
        check_status!(res);
        let result: U = res.json::<U>().await?;
        Ok(result)
    }

    pub fn into_retryable(
        self,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<U, YueError>> + 'a>> + 'a
    {
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
            Box::pin(async move {
                check_rate_limit(info.weight).await?;
                let client = HTTP_CLIENT.get().ok_or(YueError::new("客户端没有初始化"))?;
                let mut request =
                    request_builder.compose_request(client, info, param, method.clone())?;
                if method == Method::POST || method == Method::PUT {
                    if let Some(body) = body {
                        request = request.json(body);
                    }
                }
                let res = request.send().await?;
                check_status!(res);
                let result: U = res.json::<U>().await?;
                Ok(result)
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<U, YueError>> + 'a>>
        }
    }

    pub fn retry(self, builder: ExponentialBuilder) -> impl Future<Output = Result<U, YueError>> {
        // Add explicit type annotations to resolve type inference issues
        self.into_retryable().retry(builder)
    }
}

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

pub static SPOT_KLINE_COMMAND: LazyLock<RequestInfo> = LazyLock::new(|| {
    RequestInfo::from_base_path(BINANCE_API_BASE, SPOT_KLINE_PATH, false, 2).unwrap()
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

async fn check_rate_limit(weight: u32) -> Result<(), YueError> {
    let limiter = get_bn_rate_limiter(1200);
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

pub async fn execute_ping() -> Result<(), YueError> {
    let _ = execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, EmptyObject>(
        &PING_COMMAND,
        None,
        NonAuthRequestBuilder {},
    )
    .execute()
    .await?;
    Ok(())
}

pub fn execute_bn_get<'a, P, T, U>(
    info: &'a RequestInfo,
    param: Option<&'a P>,
    request_builder: T,
) -> YueRequest<'a, T, U>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body: None,
        method: Method::GET,
        _phantom: std::marker::PhantomData,
    }
}

pub fn execute_bn_post<'a, P, T, U>(
    info: &'a RequestInfo,
    param: Option<&'a P>,
    body: Option<&'a Value>,
    request_builder: T,
) -> YueRequest<'a, T, U>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body,
        method: Method::POST,
        _phantom: std::marker::PhantomData,
    }
}

pub fn execute_bn_put<'a, P, T, U>(
    info: &'a RequestInfo,
    param: Option<&'a P>,
    body: Option<&'a Value>,
    request_builder: T,
) -> YueRequest<'a, T, U>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body,
        method: Method::POST,
        _phantom: std::marker::PhantomData,
    }
}

/// Pure function for building request components. Easy to test.

#[cfg(test)]
mod tests {
    use super::{BNSecurityRequestBuilder, check_rate_limit, execute_bn_get, get_bn_rate_limiter};
    use crate::binance::bn_models::EmptyQueryParams;
    use crate::http_client::{NonAuthRequestBuilder, YueRequestBuilder, init_http_client};
    use crate::models::RequestInfo;
    use reqwest::{Client, Method};
    use std::collections::BTreeMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn setup() {
        init_http_client(None);
    }

    #[test]
    fn test_compose_request_with_valid_security_info() {
        let client = Client::new();
        let request_info =
            RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
        let builder = BNSecurityRequestBuilder {
            api_key: "test_api_key".to_string(),
            api_secret: "test_api_secret".to_string(),
        };

        let result = builder.compose_request(&client, &request_info, None, Method::GET);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(
            request.url().as_str(),
            "https://example.com/api/v3/test?signature=4c4df0c09aaefc2fe10f409703fd08d6754229e4c9b99897331efa42d8d65e47"
        );
        assert_eq!(
            request.headers().get("X-MBX-APIKEY").unwrap(),
            "test_api_key"
        );
    }

    #[test]
    fn test_compose_request_without_security_info() {
        let client = Client::new();
        let request_info =
            RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
        let builder = NonAuthRequestBuilder {};

        let result = builder.compose_request(&client, &request_info, None, Method::GET);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(request.url().as_str(), "https://example.com/api/v3/test");
        assert!(request.headers().get("X-MBX-APIKEY").is_none());
    }

    #[test]
    fn test_compose_request_with_query_params() {
        let client = Client::new();
        let request_info =
            RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
        let builder = BNSecurityRequestBuilder {
            api_key: "test_api_key".to_string(),
            api_secret: "test_api_secret".to_string(),
        };

        let result = builder.compose_request(
            &client,
            &request_info,
            Some("symbol=BTCUSDT".to_string()),
            Method::GET,
        );
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(
            request.url().as_str(),
            "https://example.com/api/v3/test?symbol=BTCUSDT&signature=e383f8d24830bb711f0e833507b66798c5936a8fedd29b51bc5403cffd0ba755"
        );
        assert_eq!(
            request.headers().get("X-MBX-APIKEY").unwrap(),
            "test_api_key"
        );
    }

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

    #[tokio::test]
    async fn test_execute_bn_get_basic() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        // Start a mock server
        let mock_server = MockServer::start().await;

        // Create a test RequestInfo
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        // Setup the mock
        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "message": "success"
            })))
            .mount(&mock_server)
            .await;

        // Execute the request
        let result: serde_json::Value = execute_bn_get::<
            EmptyQueryParams,
            NonAuthRequestBuilder,
            serde_json::Value,
        >(&request_info, None, NonAuthRequestBuilder {})
        .execute()
        .await?;

        assert_eq!(result["message"], "success");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_params() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        // Setup mock with query parameters
        Mock::given(method("GET"))
            .and(path(test_path))
            .and(wiremock::matchers::query_param("symbol", "BTCUSDT"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "symbol": "BTCUSDT",
                "price": "50000.00"
            })))
            .mount(&mock_server)
            .await;

        // Create parameters
        let mut params = BTreeMap::new();
        params.insert("symbol", "BTCUSDT".to_string());

        // Execute request with parameters
        let result: serde_json::Value =
            execute_bn_get(&request_info, Some(&params), NonAuthRequestBuilder {})
                .execute()
                .await?;

        assert_eq!(result["symbol"], "BTCUSDT");
        assert_eq!(result["price"], "50000.00");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_security() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        // Setup mock expecting security headers
        Mock::given(method("GET"))
            .and(path(test_path))
            .and(wiremock::matchers::header("X-MBX-APIKEY", "test_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "authenticated": true
            })))
            .mount(&mock_server)
            .await;

        // Execute request with security info
        let result: serde_json::Value =
            execute_bn_get::<EmptyQueryParams, BNSecurityRequestBuilder, serde_json::Value>(
                &request_info,
                None,
                BNSecurityRequestBuilder {
                    api_key: "test_key".to_string(),
                    api_secret: "test_secret".to_string(),
                },
            )
            .execute()
            .await?;

        assert_eq!(result["authenticated"], true);
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_error_handling() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1)?;

        // Setup mock returning error
        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "code": -1121,
                "msg": "Invalid symbol"
            })))
            .mount(&mock_server)
            .await;

        // Execute request and expect error
        let result = execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, serde_json::Value>(
            &request_info,
            None,
            NonAuthRequestBuilder {},
        )
        .execute()
        .await;
        assert!(result.is_err());
        Ok(())
    }
}

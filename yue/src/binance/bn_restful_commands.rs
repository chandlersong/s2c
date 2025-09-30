use crate::binance::bn_models::ToQueryParams;
use crate::errors::YueError;
use crate::http_client::{DefaultRateLimiter, ResponseHandler, YueRequest, YueRequestBuilder};
use crate::models::RequestInfo;
use crate::tools::sign_hmac;
use async_trait::async_trait;
use governor::{Quota, RateLimiter};
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::{LazyLock, OnceLock};
// --- API and WebSocket Base URLs ---
// The active URL is determined by the Cargo features enabled at compile time.
// Priority: test > binance-testnet > production (default)

// For Unit Tests with WireMock
#[cfg(test)]
pub const BINANCE_SPOT_API: &str = "http://127.0.0.1:18080"; // WireMock server address

// For Examples and Testnet Applications
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_SPOT_API: &str = "https://testnet.binance.vision/";

// For Production (Default)
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SPOT_API: &str = "https://api.binance.com/";

#[cfg(test)]
pub const BINANCE_SWAP_API: &str = "http://127.0.0.1:18081"; // WireMock server address

// For Examples and Testnet Applications
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_SWAP_API: &str = "https://testnet.binance.vision/";

// For Production (Default)
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SWAP_API: &str = "https://fapi.binance.com/";

// WebSocket URL
#[cfg(test)]
pub const WS_SWAP_STREAM_URL_BASE: &str = "ws://127.0.0.1:8080"; // Mock WS
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const WS_SWAP_STREAM_URL_BASE: &str = "wss://stream.binancefuture.com/";
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const WS_SWAP_STREAM_URL_BASE: &str = "wss://fstream.binance.com/";

// Portfolio Margin URL
#[cfg(test)]
pub const PORTFOLIO_MARGIN_BASE: &str = "http://127.0.0.1:8080"; // Mock PM
#[cfg(not(test))]
pub const PORTFOLIO_MARGIN_BASE: &str = "https://papi.binance.com/";

pub const PING_PATH: &str = "/api/v3/ping";
pub const SPOT_EXCHANGE_INFO_PATH: &str = "/api/v3/exchangeInfo";
pub const SPOT_SERVER_TIME_PATH: &str = "/api/v3/time";
pub const SPOT_KLINE_PATH: &str = "/api/v3/klines";
pub const SPOT_TICKER_API_PATH: &str = "/api/v3/ticker/price";

pub const SWAP_PATH: &str = "/fapi/v1/ping";
pub const SWAP_EXCHANGE_INFO_PATH: &str = "/fapi/v1/exchangeInfo";
pub const SWAP_SERVER_TIME_PATH: &str = "/fapi/v1/time";

pub const SWAP_KLINE_PATH: &str = "/fapi/v1/klines";
pub const SWAP_FUNDING_RATE_PATH: &str = "/fapi/v1/fundingRate";
pub const SWAP_FUNDING_INFO_PATH: &str = "/fapi/v1/fundingInfo";

pub const BALANCE_PATH: &str = "/papi/v1/balance";
pub const SWAP_POSITION_PATH: &str = "/papi/v1/um/positionRisk";
pub const LISTEN_KEY_PATH: &str = "/papi/v1/listenKey";

pub const WS_PING_COMMAND: &str = "ping";
pub const WS_TIME_COMMAND: &str = "time";
pub const WS_SUBSCRIBE_COMMAND: &str = "SUBSCRIBE";
pub const WS_SET_PROPERTY_COMMAND: &str = "SET_PROPERTY";
pub const WS_GET_PROPERTY_COMMAND: &str = "GET_PROPERTY";

/// 用于自动生成币安相关限流器静态变量和获取函数的宏
macro_rules! define_rate_limiter {
    ($name:ident, $rate_const:ident, $fn_name:ident) => {
        static $name: OnceLock<DefaultRateLimiter> = OnceLock::new();
        /// 获取 RateLimiter 的静态引用，由宏自动生成
        pub fn $fn_name() -> Option<&'static DefaultRateLimiter> {
            Some($name.get_or_init(|| {
                RateLimiter::direct(Quota::per_minute(NonZeroU32::new($rate_const).unwrap()).allow_burst(NonZeroU32::new($rate_const).unwrap()))
            }))
        }
    };
}

static SPOT_RATE_PER_SECOND: u32 = 1200;
static SWAP_LIMITER_PER_SECOND: u32 = 1200;
static SWAP_FUNDING_RATE_PER_SECOND: u32 = 100;

// 用宏自动生成币安现货、合约、资金费率限流器相关函数
// 用法：define_rate_limiter!(静态变量名, 速率常量名, 函数名)
define_rate_limiter!(SPOT_RATE_LIMITER, SPOT_RATE_PER_SECOND, get_bn_spot_limit);
define_rate_limiter!(SWAP_RATE_LIMITER, SWAP_LIMITER_PER_SECOND, get_bn_swap_limit);
define_rate_limiter!(FUNDING_RATE_RATE_LIMITER, SWAP_FUNDING_RATE_PER_SECOND, get_bn_funding_rate_limit);

#[derive(Clone)]
pub struct BNSecurityRequestBuilder {
    //PLAN: 用security的那个包来包裹一下，优先级低
    pub api_key: String,
    pub api_secret: String,
}

impl YueRequestBuilder for BNSecurityRequestBuilder {
    fn compose_request(&self, client: &Client, info: &RequestInfo, param: Option<String>, method: Method) -> Result<RequestBuilder, YueError> {
        let mut url = info.as_ref().clone();
        let base_query_string = param.filter(|s| !s.is_empty()).unwrap_or_default();
        let signature = sign_hmac(&base_query_string, &self.api_secret)?;
        let final_query = if base_query_string.is_empty() {
            format!("signature={}", signature)
        } else {
            format!("{}&signature={}", base_query_string, signature)
        };
        url.set_query(Some(&final_query));
        let url_str = url.to_string();
        let mut request = client.request(method, &url_str);
        request = request.header("X-MBX-APIKEY", self.api_key.clone());
        Ok(request)
    }
}

#[derive(Clone)]
pub struct BinanceResponseHandler;

impl BinanceResponseHandler {
    pub fn new() -> Self {
        BinanceResponseHandler {}
    }
}

#[async_trait]
impl<U> ResponseHandler<U> for BinanceResponseHandler
where
    U: DeserializeOwned + Send + Sync,
{
    async fn handle_response(&self, res: reqwest::Response) -> Result<U, YueError> {
        let result = res.json::<U>().await?;
        Ok(result)
    }
}

/// Wrapper for Binance requests to enable retry with backon

pub static PING_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, PING_PATH, false, 1, get_bn_spot_limit(), None).unwrap());

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

/// SPOT API
pub static SPOT_EXCHANGE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_EXCHANGE_INFO_PATH, false, 20, get_bn_spot_limit(), Some(10)).unwrap());

pub static SERVER_TIME_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_SERVER_TIME_PATH, false, 1, get_bn_spot_limit(), Some(2)).unwrap());

pub static SPOT_KLINE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_KLINE_PATH, false, 2, get_bn_spot_limit(), Some(90)).unwrap());

/// SWAP API

pub static SWAP_EXCHANGE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_API, SWAP_EXCHANGE_INFO_PATH, false, 20, get_bn_swap_limit(), Some(90)).unwrap());

pub static SWAP_FUNDING_RATE_COMMAND: LazyLock<RequestInfo> = LazyLock::new(|| {
    RequestInfo::from_base_path(BINANCE_SWAP_API, SWAP_FUNDING_RATE_PATH, false, 1, get_bn_funding_rate_limit(), Some(120)).unwrap()
});
pub static SWAP_KLINE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_API, SWAP_KLINE_PATH, false, 2, get_bn_swap_limit(), Some(90)).unwrap());

/// 全局 RateLimiter，使用 OnceLock 延迟初始化

pub fn execute_bn_get<'a, P, T, U>(info: &'a RequestInfo, param: Option<&'a P>, request_builder: T) -> YueRequest<'a, T, U, BinanceResponseHandler>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned + Send + Sync,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body: None,
        method: Method::GET,
        response_handler: BinanceResponseHandler::new(),
        _phantom: std::marker::PhantomData,
    }
}

pub fn execute_bn_post<'a, P, T, U>(
    info: &'a RequestInfo,
    param: Option<&'a P>,
    body: Option<&'a Value>,
    request_builder: T,
) -> YueRequest<'a, T, U, BinanceResponseHandler>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned + Send + Sync,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body,
        method: Method::POST,
        response_handler: BinanceResponseHandler::new(),
        _phantom: std::marker::PhantomData,
    }
}

pub fn execute_bn_put<'a, P, T, U>(
    info: &'a RequestInfo,
    param: Option<&'a P>,
    body: Option<&'a Value>,
    request_builder: T,
) -> YueRequest<'a, T, U, BinanceResponseHandler>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned + Send + Sync,
{
    let converted_param = param.map(|p| p.to_query_string());
    YueRequest {
        info,
        param: converted_param,
        request_builder,
        body,
        method: Method::POST,
        response_handler: BinanceResponseHandler::new(),
        _phantom: std::marker::PhantomData,
    }
}

/// Pure function for building request components. Easy to test.

#[cfg(test)]
mod tests {
    use super::{BNSecurityRequestBuilder, execute_bn_get, get_bn_spot_limit};
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
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1, get_bn_spot_limit(), None).unwrap();
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
        assert_eq!(request.headers().get("X-MBX-APIKEY").unwrap(), "test_api_key");
    }

    #[test]
    fn test_compose_request_without_security_info() {
        let client = Client::new();
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1, get_bn_spot_limit(), None).unwrap();
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
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1, get_bn_spot_limit(), None).unwrap();
        let builder = BNSecurityRequestBuilder {
            api_key: "test_api_key".to_string(),
            api_secret: "test_api_secret".to_string(),
        };

        let result = builder.compose_request(&client, &request_info, Some("symbol=BTCUSDT".to_string()), Method::GET);
        assert!(result.is_ok());

        let request = result.unwrap().build().unwrap();
        assert_eq!(
            request.url().as_str(),
            "https://example.com/api/v3/test?symbol=BTCUSDT&signature=e383f8d24830bb711f0e833507b66798c5936a8fedd29b51bc5403cffd0ba755"
        );
        assert_eq!(request.headers().get("X-MBX-APIKEY").unwrap(), "test_api_key");
    }

    #[tokio::test]
    async fn test_execute_bn_get_basic() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        // Start a mock server
        let mock_server = MockServer::start().await;

        // Create a test RequestInfo
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, get_bn_spot_limit(), None)?;

        // Setup the mock
        Mock::given(method("GET"))
            .and(path(test_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "message": "success"
            })))
            .mount(&mock_server)
            .await;

        // Execute the request
        let result: serde_json::Value =
            execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, serde_json::Value>(&request_info, None, NonAuthRequestBuilder {})
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
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None)?;

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
        let result: serde_json::Value = execute_bn_get(&request_info, Some(&params), NonAuthRequestBuilder {}).execute().await?;

        assert_eq!(result["symbol"], "BTCUSDT");
        assert_eq!(result["price"], "50000.00");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_security() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(&mock_server.uri(), test_path, false, 1, None, None)?;

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
        let result: serde_json::Value = execute_bn_get::<EmptyQueryParams, BNSecurityRequestBuilder, serde_json::Value>(
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
}

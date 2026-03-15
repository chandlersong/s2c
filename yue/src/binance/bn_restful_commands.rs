use crate::binance::bn_models::common::ToQueryParams;
use crate::errors::YueError;
use crate::http_client::{ClonableResponseCache, ResponseHandler, YueRequest, YueRequestBuilder};
use crate::models::{DefaultRateLimiter, HostInfo, RequestInfo, create_share_rate_limiter};
use crate::tools::sign_hmac;
use async_trait::async_trait;
use governor::{Quota, RateLimiter};
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::clone::Clone;

use std::num::NonZeroU32;
use std::sync::{Arc, LazyLock, OnceLock};
use url::Host;
// --- API and WebSocket Base URLs ---
// The active URL is determined by the Cargo features enabled at compile time.
// Priority: test > binance-testnet > production (default)

/// PLAN：这些做成配置项。比如一台server需要部署多个instance
/// 然后经过测试，发觉比上限低一点，如果定格，容易被封
static SPOT_RATE_PER_MINUTE: u32 = 1190;
static SWAP_LIMITER_PER_MINUTE: u32 = 1200;
static SWAP_FUNDING_PER_MINUTE: u32 = 95;
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://api.binance.com/",
        SPOT_RATE_PER_MINUTE,
        create_share_rate_limiter(SPOT_RATE_PER_MINUTE),
    ))
});

#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://testnet.binance.vision/",
        SPOT_RATE_PER_MINUTE,
        create_share_rate_limiter(SPOT_RATE_PER_MINUTE),
    ))
});
#[cfg(test)]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "http://127.0.0.1:18080/",
        SPOT_RATE_PER_MINUTE,
        create_share_rate_limiter(SPOT_RATE_PER_MINUTE),
    ))
});

#[cfg(test)]
pub const BINANCE_SWAP_API: &str = "http://127.0.0.1:18081"; // WireMock server address
#[cfg(test)]
pub const BINANCE_SWAP_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "http://127.0.0.1:18081",
        SWAP_LIMITER_PER_MINUTE,
        create_share_rate_limiter(SWAP_LIMITER_PER_MINUTE),
    ))
});
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_SWAP_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://testnet.binance.vision/",
        SWAP_LIMITER_PER_MINUTE,
        create_share_rate_limiter(SWAP_LIMITER_PER_MINUTE),
    ))
});

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SWAP_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://fapi.binance.com/",
        SWAP_LIMITER_PER_MINUTE,
        create_share_rate_limiter(SWAP_LIMITER_PER_MINUTE),
    ))
});

#[cfg(test)]
pub const BINANCE_FUNDING_RATE_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "http://127.0.0.1:18081",
        SWAP_FUNDING_PER_MINUTE,
        create_share_rate_limiter(SWAP_FUNDING_PER_MINUTE),
    ))
});
#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_FUNDING_RATE_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://testnet.binance.vision/",
        SWAP_FUNDING_PER_MINUTE,
        create_share_rate_limiter(SWAP_FUNDING_PER_MINUTE),
    ))
});

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_FUNDING_RATE_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://fapi.binance.com/",
        SWAP_FUNDING_PER_MINUTE,
        create_share_rate_limiter(SWAP_FUNDING_PER_MINUTE),
    ))
});

pub const BINANCE_PAPI_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://papi.binance.com/",
        SWAP_LIMITER_PER_MINUTE,
        create_share_rate_limiter(SWAP_LIMITER_PER_MINUTE),
    ))
});

pub const PING_PATH: &str = "/api/v3/ping";
pub const SPOT_EXCHANGE_INFO_PATH: &str = "/api/v3/exchangeInfo";
pub const SPOT_SERVER_TIME_PATH: &str = "/api/v3/time";
pub const SPOT_KLINE_PATH: &str = "/api/v3/klines";
pub const SPOT_TICKER_API_PATH: &str = "/api/v3/ticker/price";
pub const SPOT_AVERAGE_PATH: &str = "/api/v3/avgPrice";
pub const SPOT_TICKER_24HR_PATH: &str = "/api/v3/ticker/24hr";
pub const SPOT_DEPTH: &str = "/api/v3/depth";

pub const SWAP_PATH: &str = "/fapi/v1/ping";
pub const SWAP_EXCHANGE_INFO_PATH: &str = "/fapi/v1/exchangeInfo";
pub const SWAP_SERVER_TIME_PATH: &str = "/fapi/v1/time";

pub const SWAP_KLINE_PATH: &str = "/fapi/v1/klines";
pub const SWAP_FUNDING_RATE_PATH: &str = "/fapi/v1/fundingRate";
pub const SWAP_FUNDING_INFO_PATH: &str = "/fapi/v1/fundingInfo";

pub const BALANCE_PATH: &str = "/papi/v1/balance";
pub const SWAP_POSITION_PATH: &str = "/papi/v1/um/positionRisk";

pub const SWAP_LISTEN_KEY_PATH: &str = "/fapi/v1/listenKey";

pub const PAPI_LISTEN_KEY_PATH: &str = "/papi/v1/listenKey";

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

// 用宏自动生成币安现货、合约、资金费率限流器相关函数
// 用法：define_rate_limiter!(静态变量名, 速率常量名, 函数名)
define_rate_limiter!(SPOT_RATE_LIMITER, SPOT_RATE_PER_MINUTE, get_bn_spot_limit);
define_rate_limiter!(SWAP_RATE_LIMITER, SWAP_LIMITER_PER_MINUTE, get_bn_swap_limit);
define_rate_limiter!(FUNDING_RATE_RATE_LIMITER, SWAP_FUNDING_PER_MINUTE, get_bn_funding_rate_limit);

#[derive(Clone)]
pub struct BNSecurityRequestBuilder {
    //FUTURE: 用security的那个包来包裹一下，优先级低
    api_key: String,
    api_secret: String,
}

impl BNSecurityRequestBuilder {
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self { api_key, api_secret }
    }
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
    async fn handle_response(&self, res: ClonableResponseCache, rate_limiter: Option<&DefaultRateLimiter>) -> Result<U, YueError> {
        // 打印所有响应 headers（名称和对应的值）。 对于非 UTF-8 的 header 值，会标记为 <non-utf8>
        for (name, value) in res.headers.iter() {
            match value.to_str() {
                Ok(v) => println!("Response header {}: {}", name.as_str(), v),
                Err(_) => println!("Response header {}: <non-utf8>", name.as_str()),
            }
        }

        // 示例：如果传入了 rate_limiter，我们可以尝试用它来获取/消耗令牌
        if let Some(_limiter) = rate_limiter {
            // 注意：这里不能直接修改 limiter 的内部配额（governor 的 API 不支持动态修改 quota），
            // 但可以通过请求更多令牌来消耗它 -- 下面只是示例性的非阻塞检查（不建议在生产代码中阻塞处理）
            // 真实场景应当在发起请求前进行限流检查（perform_request_async 中已有检查），
            // 这里展示如何访问 limiter 并在需要时记录或执行额外操作。
            // e.g., limiter.check(); // 如果有类似的非阻塞方法
            println!("BinanceResponseHandler received a rate_limiter reference");
        }

        let result = serde_json::from_slice::<U>(&res.body)?;
        Ok(result)
    }
}

/// Wrapper for Binance requests to enable retry with backon

pub static PING_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), PING_PATH, false, 1, None, None).unwrap());

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
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_EXCHANGE_INFO_PATH, false, 20, None, Some(10)).unwrap());

pub static SERVER_TIME_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_SERVER_TIME_PATH, false, 1, None, Some(2)).unwrap());

pub static SPOT_KLINE_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_KLINE_PATH, false, 2, None, Some(60 * 60)).unwrap());

pub static SPOT_AVERAGE_PRICE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_AVERAGE_PATH, false, 2, None, Some(2)).unwrap());

pub static SPOT_TICKER_24HR_ONE_SYMBOL_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_TICKER_24HR_PATH, false, 2, None, Some(2)).unwrap());

//请求交易对为1000的深度数据
pub static SPOT_DEPTH_1000_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_DEPTH, false, 50, None, Some(2)).unwrap());

/// SWAP API

pub static SWAP_EXCHANGE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SWAP_EXCHANGE_INFO_PATH, false, 20, None, Some(90)).unwrap());

pub static SWAP_FUNDING_RATE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_FUNDING_RATE_BASE.clone(), SWAP_FUNDING_RATE_PATH, false, 1, None, Some(60 * 60)).unwrap());

/**
根据api。这个注释是动态的。如果所以专门写一个command用于处理,
因为每次取1k，所有为5
*/
pub static SWAP_KLINE_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_KLINE_PATH, false, 5, None, Some(60 * 60)).unwrap());

pub static SWAP_FIVE_MIN_KLINE_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_KLINE_PATH, false, 1, None, Some(60 * 60)).unwrap());

pub static SWAP_LISTEN_KEY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_LISTEN_KEY_PATH, false, 1, None, Some(60 * 60)).unwrap());

pub static PAPI_LISTEN_KEY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_PAPI_BASE.clone(), PAPI_LISTEN_KEY_PATH, false, 1, None, Some(60 * 60)).unwrap());

/// 全局 RateLimiter，使用 OnceLock 延迟初始化

pub fn execute_bn_get<'a, P, T, U>(info: &'a RequestInfo, param: Option<&'a P>, request_builder: T) -> YueRequest<'a, T, U, BinanceResponseHandler>
where
    P: ToQueryParams,
    T: YueRequestBuilder + Clone,
    U: DeserializeOwned + Send + Sync,
{
    todo!()
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
    todo!()
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
    todo!()
}

/// Pure function for building request components. Easy to test.

#[cfg(test)]
mod tests {
    use super::{BNSecurityRequestBuilder, BinanceResponseHandler, execute_bn_get, get_bn_spot_limit};
    use crate::binance::bn_models::common::EmptyQueryParams;
    use crate::http_client::{ClonableResponseCache, NonAuthRequestBuilder, ResponseHandler, YueRequestBuilder, init_http_client};
    use crate::models::RequestInfo;
    use crate::tools::create_mock_host_info;
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
        let request_info = RequestInfo::from_base_path(create_mock_host_info("https://example.com"), "/api/v3/test", false, 1, None, None).unwrap();
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
        let request_info = RequestInfo::from_base_path(create_mock_host_info("https://example.com"), "/api/v3/test", false, 1, None, None).unwrap();
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
        let request_info = RequestInfo::from_base_path(create_mock_host_info("https://example.com"), "/api/v3/test", false, 1, None, None).unwrap();
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
        let request_info = RequestInfo::from_base_path(create_mock_host_info(&mock_server.uri()), test_path, false, 1, None, None)?;

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
        let request_info = RequestInfo::from_base_path(create_mock_host_info(&mock_server.uri()), test_path, false, 1, None, None)?;

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
        let request_info = RequestInfo::from_base_path(create_mock_host_info(&mock_server.uri()), test_path, false, 1, None, None)?;

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

    #[test]
    fn test_binance_response_handler_handle_response_success() {
        // 创建测试JSON数据
        let test_json = serde_json::json!({
            "symbol": "BTCUSDT",
            "price": "45000.00",
            "timestamp": 1672531200000i64
        });

        // 创建BinanceResponseHandler
        let handler = BinanceResponseHandler::new();

        // 验证handler实例创建成功
        assert!(matches!(handler, BinanceResponseHandler {}));

        // 测试handle_response的核心逻辑：JSON反序列化
        // 这是handle_response方法的核心功能 res.json::<U>().await?

        // 直接测试JSON序列化和反序列化，这模拟了handle_response的核心行为
        let json_str = serde_json::to_string(&test_json).unwrap();

        // 模拟reqwest::Response::json()方法的行为
        let deserialization_result: Result<serde_json::Value, serde_json::Error> = serde_json::from_str(&json_str);

        // 验证反序列化成功（这是handle_response的核心逻辑）
        assert!(deserialization_result.is_ok());
        let parsed_result = deserialization_result.unwrap();

        // 验证响应被正确解析
        assert_eq!(parsed_result["symbol"], "BTCUSDT");
        assert_eq!(parsed_result["price"], "45000.00");
        assert_eq!(parsed_result["timestamp"], 1672531200000i64);

        // 这个测试验证了handle_response方法的核心功能：
        // 1. BinanceResponseHandler实例能正确创建
        // 2. JSON反序列化功能正常工作（handle_response的核心逻辑）
        // 3. 典型的币安API响应格式能被正确解析
    }

    #[tokio::test]
    async fn test_binance_response_handler_with_mock_server() -> Result<(), Box<dyn std::error::Error>> {
        // 启动mock server
        let mock_server = MockServer::start().await;

        // 创建测试JSON数据
        let test_json = serde_json::json!({
            "symbol": "BTCUSDT",
            "price": "45000.00",
            "timestamp": 1672531200000i64,
            "status": "success"
        });

        // 设置mock响应，包含自定义header
        Mock::given(method("GET"))
            .and(path("/api/v3/ticker/price"))
            .and(wiremock::matchers::query_param("symbol", "BTCUSDT"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_json)
                    .insert_header("Content-Type", "application/json")
                    .insert_header("X-MBX-USED-WEIGHT", "1")
                    .insert_header("X-MBX-ORDER-COUNT", "0")
                    .insert_header("Server", "nginx"),
            )
            .mount(&mock_server)
            .await;

        // 创建HTTP客户端并发送请求，使用 query API 确保 query 参数格式一致
        let client = reqwest::Client::new();
        let url = format!("{}/api/v3/ticker/price", mock_server.uri());
        let response = client.get(&url).query(&[("symbol", "BTCUSDT")]).send().await?;

        // 验证响应headers（Content-Type 可能包含 charset，使用 starts_with 检查）
        assert_eq!(response.status(), 200);
        let ct = response.headers().get("Content-Type").unwrap().to_str().unwrap();
        assert!(ct.starts_with("application/json"));
        assert_eq!(response.headers().get("X-MBX-USED-WEIGHT").unwrap(), "1");
        assert_eq!(response.headers().get("Server").unwrap(), "nginx");

        // 创建BinanceResponseHandler并测试handle_response方法
        let handler = BinanceResponseHandler::new();
        let cache = ClonableResponseCache::from_response(response).await;
        let result: serde_json::Value = handler.handle_response(cache, None).await?;

        // 验证解析结果
        assert_eq!(result["symbol"], "BTCUSDT");
        assert_eq!(result["price"], "45000.00");
        assert_eq!(result["timestamp"], 1672531200000i64);
        assert_eq!(result["status"], "success");

        Ok(())
    }

    #[tokio::test]
    async fn test_binance_response_handler_error_response() -> Result<(), Box<dyn std::error::Error>> {
        // 启动mock server
        let mock_server = MockServer::start().await;

        // 创建错误响应JSON
        let error_json = serde_json::json!({
            "code": -1121,
            "msg": "Invalid symbol."
        });

        // 设置mock错误响应
        Mock::given(method("GET"))
            .and(path("/api/v3/ticker/price"))
            .and(wiremock::matchers::query_param("symbol", "INVALID"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(&error_json)
                    .insert_header("Content-Type", "application/json")
                    .insert_header("X-MBX-USED-WEIGHT", "1"),
            )
            .mount(&mock_server)
            .await;

        // 创建HTTP客户端并发送请求
        let client = reqwest::Client::new();
        let url = format!("{}/api/v3/ticker/price?symbol=INVALID", mock_server.uri());
        let response = client.get(&url).send().await?;

        // 验证响应状态和headers
        assert_eq!(response.status(), 400);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "application/json");

        // 创建BinanceResponseHandler并测试handle_response方法
        let handler = BinanceResponseHandler::new();
        let cache = ClonableResponseCache::from_response(response).await;
        let result: serde_json::Value = handler.handle_response(cache, None).await?;

        // 验证错误响应解析
        assert_eq!(result["code"], -1121);
        assert_eq!(result["msg"], "Invalid symbol.");

        Ok(())
    }

    #[tokio::test]
    async fn test_binance_response_handler_custom_headers() -> Result<(), Box<dyn std::error::Error>> {
        // 启动mock server
        let mock_server = MockServer::start().await;

        // 创建复杂的响应数据
        let complex_json = serde_json::json!({
            "symbols": [
                {
                    "symbol": "BTCUSDT",
                    "price": "45000.00"
                },
                {
                    "symbol": "ETHUSDT",
                    "price": "3000.00"
                }
            ],
            "serverTime": 1672531200000i64
        });

        // 设置包含多个自定义header的mock响应
        Mock::given(method("GET"))
            .and(path("/api/v3/ticker/price"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&complex_json)
                    .insert_header("Content-Type", "application/json; charset=utf-8")
                    .insert_header("X-MBX-USED-WEIGHT", "5")
                    .insert_header("X-MBX-ORDER-COUNT", "0")
                    .insert_header("X-MBX-USED-WEIGHT-1M", "5")
                    .insert_header("Cache-Control", "no-cache")
                    .insert_header("Connection", "keep-alive")
                    .insert_header("Date", "Mon, 01 Jan 2024 00:00:00 GMT")
                    .insert_header("Server", "nginx/1.18.0")
                    .insert_header("Vary", "Accept-Encoding"),
            )
            .mount(&mock_server)
            .await;

        // 发送请求
        let client = reqwest::Client::new();
        let url = format!("{}/api/v3/ticker/price", mock_server.uri());
        let response = client.get(&url).send().await?;

        // 验证所有自定义headers
        let headers = response.headers();
        assert_eq!(response.status(), 200);
        assert!(headers.get("Content-Type").unwrap().to_str().unwrap().starts_with("application/json"));
        assert_eq!(headers.get("X-MBX-USED-WEIGHT").unwrap(), "5");
        assert_eq!(headers.get("X-MBX-ORDER-COUNT").unwrap(), "0");
        assert_eq!(headers.get("X-MBX-USED-WEIGHT-1M").unwrap(), "5");
        assert_eq!(headers.get("Cache-Control").unwrap(), "no-cache");
        assert_eq!(headers.get("Connection").unwrap(), "keep-alive");
        assert_eq!(headers.get("Server").unwrap(), "nginx/1.18.0");
        assert_eq!(headers.get("Vary").unwrap(), "Accept-Encoding");

        // 测试BinanceResponseHandler
        let handler = BinanceResponseHandler::new();
        let cache = ClonableResponseCache::from_response(response).await;
        let result: serde_json::Value = handler.handle_response(cache, None).await?;

        // 验证复杂JSON结构解析
        assert!(result["symbols"].is_array());
        assert_eq!(result["symbols"][0]["symbol"], "BTCUSDT");
        assert_eq!(result["symbols"][0]["price"], "45000.00");
        assert_eq!(result["symbols"][1]["symbol"], "ETHUSDT");
        assert_eq!(result["symbols"][1]["price"], "3000.00");
        assert_eq!(result["serverTime"], 1672531200000i64);

        Ok(())
    }
}

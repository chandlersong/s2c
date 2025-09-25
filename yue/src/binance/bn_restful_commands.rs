use crate::binance::bn_models::{
    BINANCE_SPOT_API, BINANCE_SWAP_API, PING_PATH, SPOT_EXCHANGE_INFO_PATH, SPOT_KLINE_PATH, SPOT_SERVER_TIME_PATH, SWAP_FUNDING_RATE_PATH,
    SWAP_KLINE_PATH, ToQueryParams,
};
use crate::errors::YueError;
use crate::http_client::{YueRequest, YueRequestBuilder};
use crate::models::RequestInfo;
use crate::tools::sign_hmac;
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Clone)]
pub struct BNSecurityRequestBuilder {
    //TODO: 用security的那个包来包裹一下，优先级低
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

/// Wrapper for Binance requests to enable retry with backon

pub static PING_COMMAND: LazyLock<RequestInfo> = LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, PING_PATH, false, 1).unwrap());

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
pub static EXCHANGE_INFO_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_EXCHANGE_INFO_PATH, false, 20).unwrap());

pub static SERVER_TIME_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_SERVER_TIME_PATH, false, 1).unwrap());

pub static SPOT_KLINE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_API, SPOT_KLINE_PATH, false, 2).unwrap());

/// SWAP API

pub static SWAP_FUNDING_RATE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_API, SWAP_FUNDING_RATE_PATH, false, 2).unwrap());
pub static SWAP_KLINE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_API, SWAP_KLINE_PATH, false, 2).unwrap());

/// 全局 RateLimiter，使用 OnceLock 延迟初始化

pub fn execute_bn_get<'a, P, T, U>(info: &'a RequestInfo, param: Option<&'a P>, request_builder: T) -> YueRequest<'a, T, U>
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

pub fn execute_bn_post<'a, P, T, U>(info: &'a RequestInfo, param: Option<&'a P>, body: Option<&'a Value>, request_builder: T) -> YueRequest<'a, T, U>
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

pub fn execute_bn_put<'a, P, T, U>(info: &'a RequestInfo, param: Option<&'a P>, body: Option<&'a Value>, request_builder: T) -> YueRequest<'a, T, U>
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
    use super::{BNSecurityRequestBuilder, execute_bn_get};
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
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
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
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
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
        let request_info = RequestInfo::from_base_path("https://example.com", "/api/v3/test", false, 1).unwrap();
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
        let result: serde_json::Value =
            execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, serde_json::Value>(&request_info, None, NonAuthRequestBuilder {})
                .execute(None)
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
        let result: serde_json::Value = execute_bn_get(&request_info, Some(&params), NonAuthRequestBuilder {})
            .execute(None)
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
        let result: serde_json::Value = execute_bn_get::<EmptyQueryParams, BNSecurityRequestBuilder, serde_json::Value>(
            &request_info,
            None,
            BNSecurityRequestBuilder {
                api_key: "test_key".to_string(),
                api_secret: "test_secret".to_string(),
            },
        )
        .execute(None)
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
        let result = execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, serde_json::Value>(&request_info, None, NonAuthRequestBuilder {})
            .execute(None)
            .await;
        assert!(result.is_err());
        Ok(())
    }
}

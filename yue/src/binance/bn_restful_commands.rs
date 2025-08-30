use crate::binance::bn_models::{
    BINANCE_API_BASE, EXCHANGE_INFO_PATH, EmptyQueryParams, PING_PATH, SERVER_TIME_PATH,
    SPOT_KLINE_PATH, SecurityInfo, ToQueryParams,
};
use crate::errors::BraavosError;
use crate::http_client::HTTP_CLIENT;
use crate::models::{EmptyObject, RequestInfo};
use crate::tools::sign_hmac;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Jitter, Quota, RateLimiter};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::num::NonZeroU32;
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;
use tokio::time::timeout;
use ureq::{Agent, Request};

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
    RequestInfo::from_base_path(BINANCE_API_BASE, SPOT_KLINE_PATH, false, 1).unwrap()
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

async fn check_rate_limit(weight: u32) -> Result<(), BraavosError> {
    let limiter = get_bn_rate_limiter(1200);
    // 超时时间：2 秒
    let timeout_duration = Duration::from_secs(2);
    // 抖动避免请求堆积
    let jitter = Jitter::up_to(Duration::from_millis(100));

    // 验证权重非零
    let weight = match NonZeroU32::new(weight) {
        Some(w) => w,
        None => return Err(BraavosError::new("权重必须为非零")),
    };
    // 等待令牌或超时
    let result = timeout(
        timeout_duration,
        limiter.until_n_ready_with_jitter(weight, jitter),
    )
    .await;
    match result {
        Ok(inner_result) => match inner_result {
            Ok(()) => Ok(()),
            Err(_) => Err(BraavosError::new("令牌不足")),
        },
        Err(_) => Err(BraavosError::new("限流超时")),
    }
}

pub async fn execute_ping() -> Result<(), BraavosError> {
    let _ = execute_bn_get::<EmptyQueryParams, EmptyObject>(&PING_COMMAND, None, None).await?;
    Ok(())
}

pub async fn execute_bn_get<P: ToQueryParams, U: DeserializeOwned>(
    info: &RequestInfo,
    param: Option<P>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT
        .get()
        .ok_or(BraavosError::new("客户端没有初始化"))?;
    let request =
        create_request_with_param_and_security(client, info, param, "GET", security_info)?;
    let res = request.call()?;
    let result: U = res.into_json()?;
    Ok(result)
}

pub async fn execute_bn_post<U: DeserializeOwned, P: ToQueryParams>(
    info: &RequestInfo,
    param: Option<P>,
    body: Option<Value>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT
        .get()
        .ok_or(BraavosError::new("客户端没有初始化"))?;
    let request =
        create_request_with_param_and_security(client, info, param, "POST", security_info)?;
    let request_body = body.unwrap_or_else(|| Value::Null);
    let res = request.send_json(&request_body)?;
    let result: U = res.into_json()?;
    Ok(result)
}

pub async fn execute_bn_put<U: DeserializeOwned, P: ToQueryParams>(
    info: &RequestInfo,
    param: Option<P>,
    body: Option<Value>,
    security_info: Option<SecurityInfo>,
) -> Result<U, BraavosError> {
    check_rate_limit(info.weight).await?;
    let client = HTTP_CLIENT
        .get()
        .ok_or(BraavosError::new("客户端没有初始化"))?;
    let request =
        create_request_with_param_and_security(client, info, param, "PUT", security_info)?;
    let request_body = body.unwrap_or_else(|| Value::Null);
    let res = request.send_json(&request_body)?;
    let result: U = res.into_json()?;
    Ok(result)
}

/// Pure function for building request components. Easy to test.
fn build_request_components<P: ToQueryParams>(
    info: &RequestInfo,
    param: Option<P>,
    security_info: Option<SecurityInfo>,
) -> (String, Option<String>) {
    let mut url = info.as_ref().clone();
    let base_query_string = param
        .as_ref()
        .map(|p| p.to_query_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let api_key = security_info.as_ref().map(|s| s.api_key.clone());
    if let Some(sec_info) = &security_info {
        let signature = sign_hmac(&base_query_string, &sec_info.api_secret).unwrap();
        let final_query = if base_query_string.is_empty() {
            format!("signature={}", signature)
        } else {
            format!("{}&signature={}", base_query_string, signature)
        };
        url.set_query(Some(&final_query));
    } else if !base_query_string.is_empty() {
        url.set_query(Some(&base_query_string));
    }
    (url.to_string(), api_key)
}

/// Imperative shell for creating the request object.
fn create_request_with_param_and_security<P: ToQueryParams>(
    client: &Agent,
    info: &RequestInfo,
    param: Option<P>,
    method: &str,
    security_info: Option<SecurityInfo>,
) -> Result<Request, BraavosError> {
    let (url, api_key) = build_request_components(info, param, security_info);
    let mut request = client.request(method, &url);
    if let Some(key) = api_key {
        request = request.set("X-MBX-APIKEY", &key);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::{build_request_components, check_rate_limit, execute_bn_get, get_bn_rate_limiter};
    use crate::binance::bn_models::{EmptyQueryParams, SecurityInfo};
    use crate::http_client::init_http_client;
    use crate::models::RequestInfo;
    use std::collections::BTreeMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn setup() {
        init_http_client(None);
    }

    #[test]
    fn test_build_request_components() {
        let request_info =
            RequestInfo::from_base_path("http://127.0.0.1:8080", "/api/v3/order", false, 1)
                .unwrap();
        let security_info = SecurityInfo {
            api_key: "test_api_key".to_string(),
            api_secret: "test_api_secret".to_string(),
        };

        // Scenario 1: No params, no security
        let (url, header) = build_request_components::<EmptyQueryParams>(&request_info, None, None);
        assert_eq!(
            url, "http://127.0.0.1:8080/api/v3/order",
            "Scenario 1 (No params, no security): URL should be base path"
        );
        assert_eq!(
            header, None,
            "Scenario 1 (No params, no security): API key header should not be set"
        );

        // Scenario 2: Params, no security
        let mut params_map = BTreeMap::new();
        params_map.insert("symbol", "BTCUSDT".to_string());
        params_map.insert("side", "BUY".to_string());
        params_map.insert("type", "LIMIT".to_string());
        let (url, header) = build_request_components(&request_info, Some(params_map.clone()), None);
        assert_eq!(
            url, "http://127.0.0.1:8080/api/v3/order?side=BUY&symbol=BTCUSDT&type=LIMIT",
            "Scenario 2 (Params, no security): URL should include sorted query parameters"
        );
        assert_eq!(
            header, None,
            "Scenario 2 (Params, no security): API key header should not be set"
        );

        // Scenario 3: No params, security
        let (url, header) = build_request_components::<EmptyQueryParams>(
            &request_info,
            None,
            Some(security_info.clone()),
        );
        assert_eq!(
            url,
            "http://127.0.0.1:8080/api/v3/order?signature=4c4df0c09aaefc2fe10f409703fd08d6754229e4c9b99897331efa42d8d65e47",
            "Scenario 3 (No params, security): URL should contain only the signature"
        );
        assert_eq!(
            header,
            Some("test_api_key".to_string()),
            "Scenario 3 (No params, security): API key header should be set"
        );

        // Scenario 4: Empty params, security
        let empty_map = BTreeMap::new();
        let (url, header) =
            build_request_components(&request_info, Some(empty_map), Some(security_info.clone()));
        assert_eq!(
            url,
            "http://127.0.0.1:8080/api/v3/order?signature=4c4df0c09aaefc2fe10f409703fd08d6754229e4c9b99897331efa42d8d65e47",
            "Scenario 4 (Empty params, security): URL should contain only the signature"
        );
        assert_eq!(
            header,
            Some("test_api_key".to_string()),
            "Scenario 4 (Empty params, security): API key header should be set"
        );

        // Scenario 5: Params, security
        let (url, header) =
            build_request_components(&request_info, Some(params_map), Some(security_info));
        assert_eq!(
            url,
            "http://127.0.0.1:8080/api/v3/order?side=BUY&symbol=BTCUSDT&type=LIMIT&signature=627ca17e230c3eb329537cdab76b3654fd620c7d863f7344f7d625c02f7cc110",
            "Scenario 5 (Params, security): URL should contain sorted params and signature"
        );
        assert_eq!(
            header,
            Some("test_api_key".to_string()),
            "Scenario 5 (Params, security): API key header should be set"
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
        let result: serde_json::Value =
            execute_bn_get::<EmptyQueryParams, serde_json::Value>(&request_info, None, None)
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
        let result: serde_json::Value = execute_bn_get(&request_info, Some(params), None).await?;

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

        let security_info = SecurityInfo {
            api_key: "test_key".to_string(),
            api_secret: "test_secret".to_string(),
        };

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
        let result: serde_json::Value = execute_bn_get::<EmptyQueryParams, serde_json::Value>(
            &request_info,
            None,
            Some(security_info),
        )
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
        let result =
            execute_bn_get::<EmptyQueryParams, serde_json::Value>(&request_info, None, None).await;
        assert!(result.is_err());
        Ok(())
    }
}

use crate::errors::YueError;
use crate::models::{HostInfo, RequestInfo, create_share_rate_limiter};
use reqwest::RequestBuilder;
use serde::de::DeserializeOwned;
use std::clone::Clone;

use crate::binance::http_client::{BinanceRestfulClient, BinanceSecurityInfo};
use log::error;
use std::sync::{Arc, LazyLock};
// --- API and WebSocket Base URLs ---
// The active URL is determined by the Cargo features enabled at compile time.
// Priority: test > binance-testnet > production (default)

/// PLAN：这些做成配置项。比如一台server需要部署多个instance
/// 然后经过测试，发觉比上限低一点，如果定格，容易被封
pub static SPOT_RATE_PER_MINUTE: u32 = 1190;
pub static SWAP_LIMITER_PER_MINUTE: u32 = 1200;
pub static SWAP_FUNDING_PER_MINUTE: u32 = 95;
#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://api.binance.com",
        SPOT_RATE_PER_MINUTE,
        create_share_rate_limiter(SPOT_RATE_PER_MINUTE),
    ))
});

#[cfg(all(feature = "binance-testnet", not(test)))]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://testnet.binance.vision",
        SPOT_RATE_PER_MINUTE,
        create_share_rate_limiter(SPOT_RATE_PER_MINUTE),
    ))
});
#[cfg(test)]
pub const BINANCE_SPOT_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "http://127.0.0.1:18080",
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
        "https://testnet.binance.vision",
        SWAP_LIMITER_PER_MINUTE,
        create_share_rate_limiter(SWAP_LIMITER_PER_MINUTE),
    ))
});

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_SWAP_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://fapi.binance.com",
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
        "https://testnet.binance.vision",
        SWAP_FUNDING_PER_MINUTE,
        create_share_rate_limiter(SWAP_FUNDING_PER_MINUTE),
    ))
});

#[cfg(not(any(feature = "binance-testnet", test)))]
pub const BINANCE_FUNDING_RATE_BASE: LazyLock<Arc<HostInfo>> = LazyLock::new(|| {
    Arc::new(HostInfo::new(
        "https://fapi.binance.com",
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
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SPOT_BASE.clone(), SPOT_EXCHANGE_INFO_PATH, false, 20, None, Some(90)).unwrap());

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
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_EXCHANGE_INFO_PATH, false, 20, None, Some(90)).unwrap());

pub static SWAP_FUNDING_RATE_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_FUNDING_RATE_BASE.clone(), SWAP_FUNDING_RATE_PATH, false, 1, None, Some(60 * 60)).unwrap());

/**
根据api。这个注释是动态的。如果所以专门写一个command用于处理,
因为每次取1k，所有为5
*/
pub static SWAP_KLINE_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_KLINE_PATH, false, 10, None, Some(60 * 60)).unwrap());

pub static SWAP_FIVE_MIN_KLINE_HISTORY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_KLINE_PATH, false, 2, None, Some(60 * 60)).unwrap());

pub static SWAP_LISTEN_KEY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_SWAP_BASE.clone(), SWAP_LISTEN_KEY_PATH, false, 1, None, Some(60 * 60)).unwrap());

pub static PAPI_LISTEN_KEY_COMMAND: LazyLock<RequestInfo> =
    LazyLock::new(|| RequestInfo::from_base_path(BINANCE_PAPI_BASE.clone(), PAPI_LISTEN_KEY_PATH, false, 1, None, Some(60 * 60)).unwrap());

/// 全局 RateLimiter，使用 OnceLock 延迟初始化
pub async fn execute_json_request<U>(
    info: &RequestInfo,
    request_builder: RequestBuilder,
    security: Option<BinanceSecurityInfo>,
) -> Result<U, YueError>
where
    U: DeserializeOwned + Send + Sync,
{
    let client = BinanceRestfulClient::new().await;
    let response = client.request(request_builder, info, security).await?;
    // Read raw bytes first and then deserialize with serde_json so that
    // JSON parse errors are returned as serde_json::Error (mapped to YueError::SerdeError)
    // instead of being wrapped only inside reqwest::Error.
    let bytes = response.bytes().await?;
    // 尝试反序列化；如果失败，则打印响应 body 以便调试，并返回 serde 错误
    match serde_json::from_slice::<U>(&bytes) {
        Ok(res) => Ok(res),
        Err(e) => {
            // 打印到 stderr，避免调试信息混入正常输出
            error!(
                "execute_json_request - failed to parse JSON, response body: {}",
                String::from_utf8_lossy(&bytes)
            );
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::execute_json_request;
    use crate::binance::http_client::{BinanceSecurityInfo, BinanceSecurityType};
    use crate::http_client::init_http_client;
    use crate::models::RequestInfo;
    use crate::tools::create_mock_host_info;
    use reqwest::Client;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn setup() {
        init_http_client(None);
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
        let client = Arc::new(Client::new());
        let rb = client.get(request_info.as_ref().as_str());
        // Execute the request
        let result: serde_json::Value = execute_json_request::<serde_json::Value>(&request_info, rb, None).await?;

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
                "price": "500200.00"
            })))
            .mount(&mock_server)
            .await;

        // Create parameters
        let client = Arc::new(Client::new());
        let rb = client.get(request_info.as_ref().as_str()).query(&[("symbol", "BTCUSDT")]);
        // Execute request with parameters
        let result: serde_json::Value = execute_json_request(&request_info, rb, None).await?;

        assert_eq!(result["symbol"], "BTCUSDT");
        assert_eq!(result["price"], "500200.00");
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_bn_get_with_security() -> Result<(), Box<dyn std::error::Error>> {
        setup();
        let mock_server = MockServer::start().await;
        let test_path = "/api/v3/test";
        let request_info = RequestInfo::from_base_path(create_mock_host_info(&mock_server.uri()), test_path, true, 1, None, None)?;

        // Setup mock expecting security headers
        Mock::given(method("GET"))
            .and(path(test_path))
            .and(wiremock::matchers::header("X-MBX-APIKEY", "test_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "authenticated": true
            })))
            .mount(&mock_server)
            .await;
        let client = Arc::new(Client::new());
        let rb = client.get(request_info.as_ref().as_str());
        let security = BinanceSecurityInfo::new("test_key", "test_secret", BinanceSecurityType::HMAC);
        // Execute request with security info
        let result: serde_json::Value = execute_json_request::<serde_json::Value>(&request_info, rb, Some(security)).await?;
        assert_eq!(result["authenticated"], true);
        Ok(())
    }
}

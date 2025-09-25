pub use crate::binance::bn_models::HistoryVo;
pub use crate::binance::bn_models::{BinanceKline, EmptyQueryParams, ExchangeInfo, ToQueryParams};
use crate::binance::bn_restful_commands::{PING_COMMAND, SPOT_EXCHANGE_COMMAND, execute_bn_get};
use crate::errors::YueError;
use crate::http_client::{DefaultRateLimiter, NonAuthRequestBuilder};
use crate::models::{EmptyObject, RequestInfo};
use async_trait::async_trait;
use backon::{BackoffBuilder, ExponentialBuilder, Retryable};
use governor::{Quota, RateLimiter};
use li::tools::time::unix_2_readable;
use log::{debug, trace};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU16, Ordering};

static SPOT_RATE_LIMITER: OnceLock<DefaultRateLimiter> = OnceLock::new();

//TODO: 做成配置，优先级低
static SPOT_RATE_LIMITER_PER_SECOND: u32 = 1200;
/// 获取 RateLimiter 的静态引用
fn get_bn_spot_rate_limit(per_second_num: u32) -> Option<&'static DefaultRateLimiter> {
    Some(SPOT_RATE_LIMITER.get_or_init(|| {
        RateLimiter::direct(Quota::per_second(NonZeroU32::new(per_second_num).unwrap()).allow_burst(NonZeroU32::new(per_second_num).unwrap()))
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSymbolInfo {
    /// 交易对符号，如 "BTCUSDT"
    pub symbol: String,
    /// 交易状态，可能的值包括：TRADING, END_OF_DAY, HALT, BREAK
    pub status: String,
    /// 基础资产，如 "BTC"
    pub base_asset: String,
    /// 报价资产，如 "USDT"
    pub quote_asset: String,
    /// 报价资产精度
    pub quote_asset_precision: i32,
    /// 支持的订单类型数组
    pub order_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HistoryInterval {
    OneSecond,
    OneMinute,
    ThreeMinutes,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    TwoHours,
    FourHours,
    SixHours,
    EightHours,
    TwelveHours,
    OneDay,
    ThreeDays,
    OneWeek,
    OneMonth,
}

impl AsRef<str> for HistoryInterval {
    fn as_ref(&self) -> &str {
        match self {
            HistoryInterval::OneSecond => "1s",
            HistoryInterval::OneMinute => "1m",
            HistoryInterval::ThreeMinutes => "3m",
            HistoryInterval::FiveMinutes => "5m",
            HistoryInterval::FifteenMinutes => "15m",
            HistoryInterval::ThirtyMinutes => "30m",
            HistoryInterval::OneHour => "1h",
            HistoryInterval::TwoHours => "2h",
            HistoryInterval::FourHours => "4h",
            HistoryInterval::SixHours => "6h",
            HistoryInterval::EightHours => "8h",
            HistoryInterval::TwelveHours => "12h",
            HistoryInterval::OneDay => "1d",
            HistoryInterval::ThreeDays => "3d",
            HistoryInterval::OneWeek => "1w",
            HistoryInterval::OneMonth => "1M",
        }
    }
}

pub trait MuteHistoryParam: ToQueryParams {
    fn initial(symbol: String, limit: u32, interval: HistoryInterval) -> Self;
    fn create_new(&self, start_time: Option<u64>, end_time: Option<u64>) -> Self;

    fn get_symbol(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KlineParams {
    pub symbol: String,
    pub interval: HistoryInterval,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub limit: Option<u32>,
}

impl KlineParams {
    pub fn new(symbol: String, limit: u32, interval: HistoryInterval) -> Self {
        Self {
            symbol,
            interval,
            start_time: None,
            end_time: None,
            limit: Some(limit),
        }
    }
}

impl MuteHistoryParam for KlineParams {
    fn get_symbol(&self) -> &str {
        &self.symbol
    }
    fn initial(symbol: String, limit: u32, interval: HistoryInterval) -> Self {
        KlineParams {
            symbol,
            interval,
            start_time: None,
            end_time: None,
            limit: Some(limit),
        }
    }

    fn create_new(&self, start_time: Option<u64>, end_time: Option<u64>) -> Self {
        KlineParams {
            symbol: self.symbol.clone(),
            interval: self.interval.clone(),
            start_time,
            end_time,
            limit: self.limit.clone(),
        }
    }
}

impl ToQueryParams for KlineParams {
    fn to_query_string(&self) -> String {
        let mut params = vec![];
        params.push(format!("symbol={}", self.symbol));
        params.push(format!("interval={}", self.interval.as_ref()));
        if let Some(start) = self.start_time {
            params.push(format!("startTime={}", start));
        }
        if let Some(end) = self.end_time {
            params.push(format!("endTime={}", end));
        }
        if let Some(limit) = self.limit {
            params.push(format!("limit={}", limit));
        }
        params.join("&")
    }
}

pub async fn execute_ping() -> Result<(), YueError> {
    let _ = execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, EmptyObject>(&PING_COMMAND, None, NonAuthRequestBuilder {})
        .execute(get_bn_spot_rate_limit(SPOT_RATE_LIMITER_PER_SECOND))
        .await?;
    Ok(())
}

/// 获取现货交易对信息
/// 按照币安的策略。如果一个币在2022年1月1日上线。那么start_time设定为2021年为1月1日。
/// 那么返回的第一个日期是2022年1月1日
///
/// TODO： 听过一个stream的接口，获得一些就返回。
///
/// # 参数
/// * `status` - 交易对状态过滤器
///   - `None` 或 `Some("TRADING")`: 只返回交易中的交易对 (默认)
///   - `Some("ALL")`: 返回所有交易对 (不进行状态过滤)
///   - `Some("HALT")`: 只返回暂停交易的交易对
///   - 其他值: 按指定状态过滤
///
/// # 返回
/// 返回符合条件的交易对信息列表，包含 symbol, status, base_asset, quote_asset_precision, order_types
pub async fn get_trading_spot_symbols(status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError> {
    let exchange_info: ExchangeInfo =
        execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, ExchangeInfo>(&SPOT_EXCHANGE_COMMAND, None, NonAuthRequestBuilder {})
            .execute(get_bn_spot_rate_limit(SPOT_RATE_LIMITER_PER_SECOND))
            .await?;

    let filter_status = status.unwrap_or("TRADING");

    let trading_symbols: Vec<TradingSymbolInfo> = if filter_status == "ALL" {
        // Return all symbols without filtering
        exchange_info
            .symbols
            .into_iter()
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset: symbol.quote_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    } else {
        // Filter by specified status
        exchange_info
            .symbols
            .into_iter()
            .filter(|symbol| symbol.status == filter_status)
            .map(|symbol| TradingSymbolInfo {
                symbol: symbol.symbol,
                status: symbol.status,
                base_asset: symbol.base_asset,
                quote_asset: symbol.quote_asset,
                quote_asset_precision: symbol.quote_asset_precision,
                order_types: symbol.order_types,
            })
            .collect()
    };

    Ok(trading_symbols)
}

#[async_trait]
pub trait HistoryFetcher<T, O>
where
    T: MuteHistoryParam + ToQueryParams + Send + Sync,
    O: HistoryVo,
{
    async fn get_all_kline_data(&self, base_param: T, start_time: Option<u64>) -> Result<(Vec<O>, u16), YueError>;
}

#[derive(Debug, Clone)]
pub struct SimpleHistoryFetcher {
    request_info: RequestInfo,
}

impl SimpleHistoryFetcher {
    pub fn new(request_info: &RequestInfo) -> Self {
        Self {
            request_info: request_info.clone(),
        }
    }
}

#[async_trait]
impl<'a, T, O> HistoryFetcher<T, O> for SimpleHistoryFetcher
where
    T: MuteHistoryParam + ToQueryParams + Send + Sync + 'static,
    O: HistoryVo + Send,
{
    /// 获取指定交易对和时间间隔的K线数据
    ///
    /// 注意点
    /// 1. 最后一段时间最好废弃。比如说现在是11:30:00， interval是1h。那么最后一段就是11点到12点的一段时间。
    /// # 参数
    /// * `symbol` - 交易对符号，如 "BTCUSDT"
    /// * `interval` - K线时间间隔
    /// * `start_time` - 开始时间（毫秒时间戳），如果为None则获取全部历史数据
    ///
    /// # 返回
    /// 返回K线数据列表，由于API限制，每次最多1000条，会自动分页获取
    async fn get_all_kline_data(&self, base_param: T, start_time: Option<u64>) -> Result<(Vec<O>, u16), YueError> {
        let mut res: Vec<O> = Vec::new();
        let mut current_start_time = start_time;
        let request_builder = NonAuthRequestBuilder {};
        let retry_count = AtomicU16::new(0);
        let symbol = base_param.get_symbol();
        loop {
            let params = base_param.create_new(current_start_time, None);
            let retry_policy = ExponentialBuilder::default()
                .with_jitter() // 添加随机抖动
                .with_factor(1.5) // 指数因子 1.5
                .with_max_times(10)
                .with_min_delay(std::time::Duration::from_millis(100)) // 最小延迟 500ms
                .with_max_delay(std::time::Duration::from_secs(10))
                .build();
            let klines: Vec<O> = execute_bn_get::<T, NonAuthRequestBuilder, Vec<O>>(&self.request_info, Some(&params), request_builder.clone())
                .into_retryable(get_bn_spot_rate_limit(SPOT_RATE_LIMITER_PER_SECOND))
                .retry(retry_policy)
                .notify(|_err, _dur| {
                    retry_count.fetch_add(1, Ordering::SeqCst); // 每次重试加 1
                })
                .await?;

            if let Some(last_kline) = klines.last() {
                if current_start_time.is_some() && current_start_time.unwrap() == last_kline.get_close_time() {
                    break;
                }
                current_start_time = Some(last_kline.get_close_time());
            } else {
                break;
            }

            trace!("{} fetch {} kline", symbol, klines.len());
            let klines_count = klines.len();
            res.extend(klines);

            if klines_count < 1000 {
                break;
            }
        }

        debug!(
            "{} fetch {} kline,from {} to {}",
            symbol,
            res.len(),
            unix_2_readable(&res.first().unwrap().get_open_time()),
            unix_2_readable(&res.last().unwrap().get_open_time())
        );
        Ok((res, retry_count.load(Ordering::SeqCst)))
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::BinanceKline;
    use crate::binance::bn_restful_commands::SPOT_KLINE_COMMAND;
    use crate::binance::history_data::{HistoryFetcher, HistoryInterval, KlineParams, SimpleHistoryFetcher};
    use crate::errors::YueError;
    use crate::http_client::init_http_client;
    use serde_json::json;
    use serial_test::serial;
    use std::net::TcpListener;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Helper function to create mock Kline data
    fn create_mock_kline(open_time: u64, close_time: u64) -> serde_json::Value {
        json!([
            open_time,  // open_time
            "10000.0",  // open
            "10100.0",  // high
            "9900.0",   // low
            "10050.0",  // close
            "10.0",     // volume
            close_time, // close_time
            "100500.0", // quote_asset_volume
            100,        // number_of_trades
            "5.0",      // taker_buy_base_asset_volume
            "50000.0",  // taker_buy_quote_asset_volume
            "0"         // ignore
        ])
    }

    async fn create_net_work() -> MockServer {
        init_http_client(None);
        let listener = TcpListener::bind("127.0.0.1:18080").expect("bind failed");
        let mock_server = MockServer::builder().listener(listener).start().await;
        mock_server
    }

    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_normal_case() {
        let mock_server = create_net_work().await;
        // Mock response with 500 klines
        let mut mock_klines = vec![];
        for i in 0..500 {
            let open_time = 1609459200000 + i * 3600000; // 1 hour intervals
            let close_time = open_time + 3600000 - 1;
            mock_klines.push(create_mock_kline(open_time, close_time));
        }

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_klines))
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, None).await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        let (kline, _) = kline_res.unwrap();
        assert_eq!(kline.len(), 500);
        assert_eq!(kline[0].open_time, 1609459200000);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_pagination() {
        let mock_server = create_net_work().await;

        // First response: 1000 klines
        let mut first_batch = vec![];
        for i in 0..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1;
            first_batch.push(create_mock_kline(open_time, close_time));
        }

        // Second response: 200 klines
        let mut second_batch = vec![];
        for i in 1000..1200 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1;
            second_batch.push(create_mock_kline(open_time, close_time));
        }

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .and(query_param("startTime", "1609459200000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(first_batch))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .and(query_param("startTime", "1613059199999")) // close_time of last in first batch
            .respond_with(ResponseTemplate::new(200).set_body_json(second_batch))
            .expect(1)
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(1609459200000)).await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        let (kline, _) = kline_res.unwrap();
        assert_eq!(kline.len(), 1200);
        assert_eq!(kline[0].open_time, 1609459200000);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_api_error() {
        let mock_server = create_net_work().await;

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(1609459200000)).await;
        assert!(kline_res.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_exactly_1000() {
        let mock_server = create_net_work().await;

        let mut mock_klines = vec![];
        for i in 0..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1;
            mock_klines.push(create_mock_kline(open_time, close_time));
        }

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_klines))
            .expect(2) // Only one request
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(1609459200000)).await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        let (kline, _) = kline_res.unwrap();
        assert_eq!(kline.len(), 1000);
        assert_eq!(kline[0].open_time, 1609459200000);
    }

    #[tokio::test]
    #[serial]
    async fn test_get_stop() {
        // 测试正好1000个
        let mock_server = create_net_work().await;

        let mut mock_klines = vec![];
        for i in 0..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1;
            mock_klines.push(create_mock_kline(open_time, close_time));
        }

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_klines))
            .expect(2) // Only one request
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(1609459200000)).await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        let (kline, _) = kline_res.unwrap();
        assert_eq!(kline.len(), 1000);
        assert_eq!(kline[0].open_time, 1609459200000);
    }
}

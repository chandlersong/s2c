pub use crate::binance::bn_models::HistoryVo;
pub use crate::binance::bn_models::{BinanceKline, EmptyQueryParams, ExchangeInfo, ToQueryParams};
use crate::binance::bn_models::{ExchangeInfoTrait, SwapExchangeInfo, SymbolInfoTrait};
use crate::binance::bn_restful_commands::{PING_COMMAND, SPOT_EXCHANGE_COMMAND, SWAP_EXCHANGE_COMMAND, execute_bn_get};
use crate::errors::YueError;
use crate::http_client::NonAuthRequestBuilder;
use crate::models::{EmptyObject, RequestInfo};
use async_trait::async_trait;
use backon::{BackoffBuilder, ExponentialBuilder, Retryable};
use li::tools::time::{ONE_SECOND_MS, unix_2_readable};
use log::debug;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU16, Ordering};

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
    /// 类型字段，spot统一填"spot"，swap填contract_type
    pub symbol_type: String,
    /// 上线时间，单位毫秒时间戳，spot取不到，所以为None，swap有值
    pub on_board_time: Option<u64>,
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

    fn get_symbol(&self) -> &str {
        &self.symbol
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
        .execute()
        .await?;
    Ok(())
}

/// 通用获取交易对信息方法，支持现货和合约
pub fn extract_trading_symbols<S: SymbolInfoTrait>(symbols: &[S], status: Option<&str>) -> Vec<TradingSymbolInfo> {
    let filter_status = status.unwrap_or("TRADING");
    symbols
        .iter()
        .filter(|symbol| filter_status == "ALL" || symbol.status() == filter_status)
        .map(|symbol| TradingSymbolInfo {
            symbol: symbol.symbol().to_string(),
            status: symbol.status().to_string(),
            base_asset: symbol.base_asset().to_string(),
            quote_asset: symbol.quote_asset().to_string(),
            quote_asset_precision: symbol.quote_precision(),
            order_types: symbol.order_types().clone(),
            symbol_type: symbol.symbol_type().to_string(),
            on_board_time: symbol.get_on_board_time(),
        })
        .collect()
}

/// 统一异步获取交易对信息
async fn get_trading_symbols<E, S, F>(fetch: F, status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError>
where
    E: ExchangeInfoTrait<SymbolInfo = S>,
    S: SymbolInfoTrait,
    F: Future<Output = Result<E, YueError>>,
{
    let exchange_info = fetch.await?;
    Ok(extract_trading_symbols(exchange_info.symbols(), status))
}

/// 获取现货交易对信息
pub async fn get_trading_spot_symbols(status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError> {
    get_trading_symbols(
        execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, ExchangeInfo>(&SPOT_EXCHANGE_COMMAND, None, NonAuthRequestBuilder {}).execute(),
        status,
    )
    .await
}

pub const CONTRACT_TYPE_PERPETUAL: &str = "PERPETUAL";

/// 获取合约交易对信息
/// PERPETUAL 为永续
/// CURRENT_QUARTER：为下一季
/// NEXT_QUARTER：当前季度合约
pub async fn get_trading_swap_symbols(status: Option<&str>, type_filter: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError> {
    let all = get_trading_symbols(
        execute_bn_get::<EmptyQueryParams, NonAuthRequestBuilder, SwapExchangeInfo>(&SWAP_EXCHANGE_COMMAND, None, NonAuthRequestBuilder {}).execute(),
        status,
    )
    .await?;
    if let Some(filter) = type_filter {
        Ok(all.into_iter().filter(|s| s.symbol_type == filter).collect())
    } else {
        Ok(all)
    }
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
    O: HistoryVo + Send + Sync + 'static,
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
        debug!("start fetch {} kline data from {:?}", symbol, start_time);
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
                .into_retryable()
                .retry(retry_policy)
                .notify(|_err, _dur| {
                    retry_count.fetch_add(1, Ordering::SeqCst); // 每次重试加 1
                })
                .await?;

            if let Some(last_kline) = klines.last() {
                if current_start_time.is_some() && current_start_time.unwrap() == last_kline.get_close_time() + ONE_SECOND_MS {
                    break;
                }
                current_start_time = Some(last_kline.get_close_time() + ONE_SECOND_MS);
            } else {
                break;
            }
            // 1745467200003
            debug!("{} fetch {} kline", symbol, klines.len());
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
    use crate::binance::bn_restful_commands::SPOT_KLINE_HISTORY_COMMAND;
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
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
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
            .and(query_param("startTime", "1613059200999")) // close_time of last in first batch
            .respond_with(ResponseTemplate::new(200).set_body_json(second_batch))
            .expect(1)
            .mount(&mock_server)
            .await;
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
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
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
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
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
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
        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = KlineParams::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(1609459200000)).await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        let (kline, _) = kline_res.unwrap();
        assert_eq!(kline.len(), 1000);
        assert_eq!(kline[0].open_time, 1609459200000);
    }
}

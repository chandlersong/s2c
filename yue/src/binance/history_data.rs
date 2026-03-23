use crate::binance::bn_models::common::{ExchangeInfoTrait, HistoryVo, SymbolInfoTrait, ToRequestBuilder};
use crate::binance::bn_models::spot_restful::ExchangeInfo;
use crate::binance::bn_models::swap_restful::SwapExchangeInfo;
use crate::binance::bn_restful_commands::{PING_COMMAND, execute_json_request};
use crate::errors::YueError;
use crate::http_client::{HTTP_CLIENT, get_http_client};
use crate::models::{EmptyObject, HistoryInterval, RequestInfo};
use crate::query_message::BatchInsert;
use actix::Recipient;
use async_trait::async_trait;
use li::tools::time::{ONE_MILL_SECOND_MS, unix_2_readable};
use log::{debug, error};
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub trait MuteHistoryParam: ToRequestBuilder {
    fn initial(symbol: String, limit: u32, interval: HistoryInterval) -> Self;
    fn create_new(&self, start_time: Option<u64>, end_time: Option<u64>, interval: Option<HistoryInterval>) -> Self;

    fn get_symbol(&self) -> &str;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommonRequestBuilder {
    pub symbol: String,
    pub interval: Option<HistoryInterval>,
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub limit: Option<u32>,
}

impl CommonRequestBuilder {
    pub fn new(symbol: String, limit: u32, interval: HistoryInterval) -> Self {
        Self {
            symbol,
            interval: Some(interval),
            start_time: None,
            end_time: None,
            limit: Some(limit),
        }
    }

    pub fn only_symbol(symbol: String) -> Self {
        Self {
            symbol,
            interval: None,
            start_time: None,
            end_time: None,
            limit: None,
        }
    }

    pub fn symbol_and_limit(symbol: String, limit: u32) -> Self {
        Self {
            symbol,
            interval: None,
            start_time: None,
            end_time: None,
            limit: Some(limit),
        }
    }
}

impl ToRequestBuilder for CommonRequestBuilder {
    fn to_request_builder(&self, request_info: &RequestInfo) -> RequestBuilder {
        let client = get_http_client();
        let res = client.get(request_info.as_ref().as_str());
        let mut params = vec![];
        params.push(("symbol", self.symbol.clone()));
        if let Some(interval) = self.interval.as_ref() {
            params.push(("interval", interval.as_ref().to_string()));
        }
        if let Some(start) = self.start_time {
            params.push(("startTime", start.to_string()));
        }
        if let Some(end) = self.end_time {
            params.push(("endTime", end.to_string()));
        }
        if let Some(limit) = self.limit {
            params.push(("limit", limit.to_string()));
        }
        res.query(&params)
    }
}

impl MuteHistoryParam for CommonRequestBuilder {
    fn initial(symbol: String, limit: u32, interval: HistoryInterval) -> Self {
        CommonRequestBuilder {
            symbol,
            interval: Some(interval),
            start_time: None,
            end_time: None,
            limit: Some(limit),
        }
    }
    fn create_new(&self, start_time: Option<u64>, end_time: Option<u64>, interval: Option<HistoryInterval>) -> Self {
        let actual_interval = interval.or_else(|| self.interval.clone());
        CommonRequestBuilder {
            symbol: self.symbol.clone(),
            interval: actual_interval,
            start_time,
            end_time,
            limit: self.limit.clone(),
        }
    }

    fn get_symbol(&self) -> &str {
        &self.symbol
    }
}

pub async fn execute_ping() -> Result<(), YueError> {
    let client = HTTP_CLIENT.get().ok_or(YueError::new("客户端没有初始化"))?;
    let rb = client.get(PING_COMMAND.as_ref().as_str());
    let _ = execute_json_request::<EmptyObject>(&PING_COMMAND, rb, None).await?;
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

/// 获取现货交易对信息
pub async fn get_trading_spot_symbols(exchange: ExchangeInfo, status: Option<&str>) -> Result<Vec<TradingSymbolInfo>, YueError> {
    Ok(extract_trading_symbols(&exchange.symbols, status))
}

pub const CONTRACT_TYPE_PERPETUAL: &str = "PERPETUAL";

/// 获取合约交易对信息
/// PERPETUAL 为永续
/// CURRENT_QUARTER：为下一季
/// NEXT_QUARTER：当前季度合约
pub async fn get_trading_swap_symbols(
    exchange: SwapExchangeInfo,
    status: Option<&str>,
    type_filter: Option<&str>,
) -> Result<Vec<TradingSymbolInfo>, YueError> {
    let all = extract_trading_symbols(exchange.symbols(), status);
    if let Some(filter) = type_filter {
        Ok(all.into_iter().filter(|s| s.symbol_type == filter).collect())
    } else {
        Ok(all)
    }
}

#[async_trait]
pub trait HistoryFetcher<T, O>
where
    T: MuteHistoryParam + ToRequestBuilder + Send + Sync,
    O: HistoryVo,
{
    ///
    /// 因为不同的情况不同。
    /// 1. 比如说因为压力大，所以失败后，retry就好了。
    /// 2. 但是如果说其他场景，比如实时查询等场景，失败了就不需要重试了。直接报错。
    ///
    async fn get_all_kline_data(
        &self,
        base_param: T,
        interval: Option<HistoryInterval>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        saver: Recipient<BatchInsert<O>>,
        retry_on_error: bool,
    ) -> Result<u64, YueError>;
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
    T: MuteHistoryParam + ToRequestBuilder + Send + Sync + 'static,
    O: HistoryVo + Send + Sync + 'static,
{
    /// 获取指定交易对和时间间隔的K线数据
    ///
    /// 大致流程：
    /// 1. 判断end_time是否为None，如果是None则设置为当前时间。
    /// 2. 分别通过interval的，更新最近和的开始时间和结束时间。然后结束时间+1ms。
    /// 3. 根据interval分段获取历数据。
    ///
    /// 注意点
    /// 1. 最后一段时间最好废弃。比如说现在是11:30:00， interval是1h。那么最后一段就是11点到12点的一段时间。
    ///
    ///
    /// # 参数
    /// * `base_param` - 输入请求的基本参数，其应该包含symbol，limit，interval等信息。主要因为多次请求，会需要开始和结束时间。
    /// * `interval` - K线时间间隔，默认值为5分钟
    /// * `start_time` - 开始时间（毫秒时间戳），如果为None则获取全部历史数据，默认值为2021年1月1日
    /// * `end_time` - 结束时间（毫秒时间戳），如果为None则表示是现在，默认值为当前时间
    ///
    /// # 返回
    /// 返回K线数据列表，由于API限制，每次最多1000条，会自动分页获取
    async fn get_all_kline_data(
        &self,
        base_param: T,
        interval: Option<HistoryInterval>,
        start_time: Option<u64>,
        end_time: Option<u64>,
        saver: Recipient<BatchInsert<O>>,
        retry_on_error: bool,
    ) -> Result<u64, YueError> {
        let symbol = base_param.get_symbol();
        let mut error_count = 0;
        // 步骤1：判断end_time是否为None，如果是则设置为当前时间
        let actual_end_time = if let Some(end) = end_time {
            end
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| YueError::new(&format!("获取当前时间失败: {}", e)))?
                .as_millis() as u64
        };

        debug!("start fetch {} kline data from {:?} to {:?}", symbol, start_time, actual_end_time);

        // 步骤2：如果没有显式传入 interval，则默认使用 5 分钟；
        // 使用选定的 interval 调整开始/结束时间到 interval 边界，然后结束时间+1ms
        let chosen_interval = if let Some(iv) = &interval {
            iv.clone()
        } else {
            HistoryInterval::FiveMinutes
        };

        let adjusted_start_time = start_time.map(|st| chosen_interval.get_close_unix_ms(st));
        let adjusted_end_time = chosen_interval.get_close_unix_ms(actual_end_time) + chosen_interval.to_milliseconds() - 1;

        debug!(
            "adjusted time range: {:?} to {} (using interval {})",
            adjusted_start_time,
            adjusted_end_time,
            chosen_interval.as_ref()
        );
        let mut total_count: u64 = 0;
        // 步骤3：根据interval分段获取历史数据
        let mut current_start_time = adjusted_start_time;
        let mut last_timestamp: Option<u64> = None;
        loop {
            // 检查是否已经超过结束时间
            if let Some(current_start) = &current_start_time {
                if current_start > &adjusted_end_time {
                    break;
                }
            }

            // 将调整好的 adjusted_end_time 传入请求参数，保证服务端返回的数据不超过期望的 endTime
            let params = base_param.create_new(current_start_time, Some(adjusted_end_time), interval.clone());

            let klines: Vec<O> = match execute_json_request::<Vec<O>>(&self.request_info, params.to_request_builder(&self.request_info), None).await {
                Ok(res) => res,
                Err(e) => {
                    error!(
                        "error symbol {} from {} when fetch data, error: {:?}",
                        symbol,
                        unix_2_readable(&current_start_time.unwrap()),
                        e
                    );
                    error_count = error_count + 1;
                    if error_count > 5 {
                        return Err(e);
                    }
                    continue;
                }
            };

            if klines.is_empty() {
                break;
            }

            let kline_num = klines.len();
            // 注意点：废弃所有非close的kline（close_time不符合 interval 倍数）
            let filtered_klines: Vec<O> = {
                let iv_ms = chosen_interval.to_milliseconds();
                klines
                    .into_iter()
                    .filter(|kline| {
                        let close_time = kline.get_close_time() + ONE_MILL_SECOND_MS;
                        // 检查 close_time 是否在 interval 边界上
                        close_time % iv_ms == 0
                    })
                    .collect()
            };
            let klines_count = filtered_klines.len() as u64;
            debug!("{} fetch {} kline, after filtered {} kline", symbol, kline_num, filtered_klines.len());
            last_timestamp = Some(filtered_klines.last().unwrap().get_close_time().clone());
            // 发送数据到 saver
            let message = BatchInsert::new(Some(symbol.to_string()), filtered_klines);
            match saver.send(message).await {
                Ok(_) => {}
                Err(e) => {
                    error_count += 1;
                    if error_count > 100 {
                        error!("Failed to send data to saver after 100 attempts, error: {:?}", e);
                    }
                    if retry_on_error {
                        continue;
                    } else {
                        return Err(YueError::new(&format!("Failed to send data to saver, error: {:?}", e)));
                    }
                }
            }

            total_count = total_count + klines_count;
            if klines_count < 1000 {
                break;
            }

            // 更新下一次的开始时间为最后一条kline的close_time + 1ms
            if let Some(k) = &last_timestamp {
                current_start_time = Some(k + ONE_MILL_SECOND_MS);
            } else {
                break;
            }
        }

        debug!(
            "{} fetch {} kline,  to {}",
            symbol,
            total_count,
            unix_2_readable(&last_timestamp.unwrap_or(0))
        );
        Ok(total_count)
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::bn_models::spot_restful::BinanceKline;
    use crate::binance::bn_restful_commands::SPOT_KLINE_HISTORY_COMMAND;
    use crate::binance::history_data::{CommonRequestBuilder, HistoryFetcher, SimpleHistoryFetcher};
    use crate::errors::YueError;
    use crate::http_client::init_http_client;
    use crate::models::HistoryInterval;
    use crate::query_message::BatchInsert;
    use actix::{Actor, Context, Recipient};
    use serde_json::json;
    use serial_test::serial;
    use std::net::TcpListener;
    use tokio::sync::mpsc;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct MockSaver;

    impl Actor for MockSaver {
        type Context = Context<Self>;
    }

    impl<O> actix::Handler<BatchInsert<O>> for MockSaver
    where
        O: Send + 'static,
    {
        type Result = Result<usize, YueError>;

        fn handle(&mut self, _msg: BatchInsert<O>, _ctx: &mut Self::Context) -> Self::Result {
            Ok(1) // 模拟成功处理，返回插入了1条记录
        }
    }

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

    /// 测试：获取K线数据基本功能
    ///
    /// 设计思路：验证get_all_kline_data能够正确获取指定数量的K线数据
    ///
    /// 场景说明：
    /// - 模拟返回500条K线数据（1小时间隔）
    /// - 调用时传入interval参数，验证能够正确处理
    /// - 验证返回的K线数量和开始时间正确
    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_normal_case() {
        let mock_server = create_net_work().await;
        // Mock response with 500 klines，close_time对齐到1h边界
        let mut mock_klines = vec![];
        for i in 0..500 {
            let open_time = 1609459200000 + i * 3600000; // 1 hour intervals
            let close_time = open_time + 3600000 - 1; // 对齐到1h边界
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
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let saver_addr = MockSaver {}.start();
        let recipient: Recipient<BatchInsert<BinanceKline>> = saver_addr.recipient();
        let kline_num: Result<u64, YueError> = fetcher
            .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), None, None, recipient, false)
            .await;
        assert!(kline_num.is_ok(), "获取K线数据失败: {:?}", kline_num.as_ref().err());
        assert_eq!(kline_num.unwrap(), 500);
    }

    /// 测试：分页获取K线数据
    ///
    /// 设计思路：验证当API返回超过1000条时的分页逻辑，确保：
    /// 1. 第一次请求获取1000条K线
    /// 2. 第二次请求从第一次的最后一条开始（+1ms）
    /// 3. 所有K线被正确合并
    ///
    /// 场景说明：
    /// - 第一批：1000条K线（close_time对齐到1h边界）
    /// - 第二批：200条K线（继续1h间隔）
    /// - 验证总共获取1200条K线
    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_pagination() {
        let mock_server = create_net_work().await;

        // First response: 1000 klines with close_time aligned to 1h boundary
        let mut first_batch = vec![];
        for i in 0..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1; // 对齐到1h边界
            first_batch.push(create_mock_kline(open_time, close_time));
        }

        // Second response: 200 klines
        let mut second_batch = vec![];
        for i in 1000..1200 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1; // 对齐到1h边界
            second_batch.push(create_mock_kline(open_time, close_time));
        }

        // 计算用于 mock 的 startTime：第一次请求 start 为第一个 open 的值（和调用时传入一致）
        let base_open = 1609459200000u64;
        let interval_ms = 3600000u64; // 1h
        let first_start = base_open;
        let second_start = base_open + (first_batch.len() as u64) * interval_ms;
        let close_time = base_open + 1200 * 3600000; // 最后一条的 close_time
        let adjusted_end_time = HistoryInterval::OneHour.get_close_unix_ms(close_time) + HistoryInterval::OneHour.to_milliseconds() - 1;

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("startTime", &first_start.to_string()))
            .and(query_param("endTime", &adjusted_end_time.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(first_batch))
            .expect(1)
            .mount(&mock_server)
            .await;

        // 第二次请求的 startTime 应该是第一次批次最后一条 kline 的 close_time + 1ms
        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("startTime", &second_start.to_string()))
            .and(query_param("endTime", &adjusted_end_time.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(second_batch))
            .expect(1)
            .mount(&mock_server)
            .await;

        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let saver_addr = MockSaver {}.start();
        let recipient: Recipient<BatchInsert<BinanceKline>> = saver_addr.recipient();
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(
                base_param,
                Some(HistoryInterval::OneHour),
                Some(1609459200000),
                Some(close_time),
                recipient,
                false,
            )
            .await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        assert_eq!(kline_res.unwrap(), 1200);
    }

    /// 测试：API返回错误时的处理
    ///
    /// 设计思路：验证当API返回错误响应时，函数能够正确返回错误
    ///
    /// 场景说明：
    /// - API返回500错误
    /// - 验证函数返回Err结果
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
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let saver_addr = MockSaver {}.start();
        let recipient: Recipient<BatchInsert<BinanceKline>> = saver_addr.recipient();
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), Some(1609459200000), None, recipient, false)
            .await;
        assert!(kline_res.is_err());
    }

    /// 测试：恰好返回1000条K线时的处理
    ///
    /// 设计思路：验证当API返回恰好1000条K线时的分页停止逻辑
    /// - 如果返回1000条，应该继续请求下一批（可能还有更多数据）
    /// - 下一次请求如果返回少于1000条，则停止
    ///
    /// 场景说明：
    /// - 第一次请求返回1000条K线
    /// - 第二次请求返回空或少于1000条，停止
    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_exactly_1000() {
        let mock_server = create_net_work().await;

        let mut mock_klines = vec![];
        for i in 0..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1; // 对齐到1h边界
            mock_klines.push(create_mock_kline(open_time, close_time));
        }

        let empty_response: Vec<serde_json::Value> = vec![];

        // 对于恰好 1000 条的场景，第二次请求应从第一批最后一条 close_time + 1ms 开始
        let base_open = 1609459200000u64;
        let close_time = base_open + (mock_klines.len() as u64) * 3600000; // 最后一条的 close_time
        let adjusted_end_time = HistoryInterval::OneHour.get_close_unix_ms(close_time) + HistoryInterval::OneHour.to_milliseconds() - 1;
        let interval_ms = 3600000u64; // 1h
        let first_start = base_open;
        let second_start = base_open + (mock_klines.len() as u64) * interval_ms;

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("startTime", &first_start.to_string()))
            .and(query_param("endTime", &adjusted_end_time.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_klines))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("startTime", &second_start.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let (tx, _rx) = mpsc::channel::<Result<(String, Vec<BinanceKline>), YueError>>(100);
        let saver_addr = MockSaver {}.start();
        let recipient: Recipient<BatchInsert<BinanceKline>> = saver_addr.recipient();
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(
                base_param,
                Some(HistoryInterval::OneHour),
                Some(1609459200000),
                Some(close_time),
                recipient,
                false,
            )
            .await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        assert_eq!(kline_res.unwrap(), 1000);
    }

    /// 测试：废弃非close的K线数据
    ///
    /// 设计思路：验证当API返回的K线中有非close的数据时（close_time不符合interval边界），
    /// 这些数据应该被过滤掉，只返回符合interval边界的K线
    ///
    /// 场景说明：
    /// - API返回1000条K线，其中：
    ///   - 900条K线的close_time对齐到1h边界（保留）
    ///   - 100条K线的close_time不对齐（废弃）
    /// - 验证最终只返回900条K线
    #[tokio::test]
    #[serial]
    async fn test_get_all_kline_data_discard_non_closed_kline() {
        let mock_server = create_net_work().await;

        let mut mock_klines = vec![];
        // 添加900条对齐的K线（close_time在1h边界上）
        for i in 0..900 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 1; // 对齐到1h边界
            mock_klines.push(create_mock_kline(open_time, close_time));
        }
        // 添加100条未对齐的K线（close_time不在1h边界上）
        for i in 900..1000 {
            let open_time = 1609459200000 + i * 3600000;
            let close_time = open_time + 3600000 - 500 - 1; // 不对齐，提前500ms
            mock_klines.push(create_mock_kline(open_time, close_time));
        }

        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_klines))
            .expect(1) // Only one request expected
            .mount(&mock_server)
            .await;

        let fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let saver_addr = MockSaver {}.start();
        let recipient: Recipient<BatchInsert<BinanceKline>> = saver_addr.recipient();
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), Some(1609459200000), None, recipient, false)
            .await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        // 应该只返回900条对齐的K线，100条未对齐的被废弃
        let num = kline_res.unwrap();
        assert_eq!(num, 900, "应该只返回900条对齐的K线，但返回了{}", num);
    }
}

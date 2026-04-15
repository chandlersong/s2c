///
/// 主要是集中了很多调用restful的过程。
///
use crate::binance::bn_models::common::{ExchangeInfoTrait, HistoryVo, SymbolInfo, SymbolInfoTrait, ToRequestBuilder};
use crate::binance::bn_models::spot_restful::ExchangeInfo;
use crate::binance::bn_models::swap_restful::SwapExchangeInfo;
use crate::binance::bn_restful_commands::{PING_COMMAND, execute_json_request};
use crate::errors::YueError;
use crate::http_client::{HTTP_CLIENT, get_http_client};
use crate::models::{EmptyObject, HistoryInterval, RequestInfo};
use crate::query_message::{BatchInsertPayload, DataSourceExecutor, DataSourceExecutorTrait, QueryCommand};
use actix::dev::MessageResponse;
use async_trait::async_trait;
use governor::Jitter;
use li::tools::time::{ONE_MILL_SECOND_MS, unix_2_readable, unix_time_now_u64_utc};
use log::{debug, error, warn};
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const ALL_TYPE: &str = "ALL";

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

/// 把symbol换成app标准的状态。
pub fn translate_symbols<S: SymbolInfoTrait>(symbols: &[S]) -> Vec<SymbolInfo> {
    symbols
        .iter()
        .map(|symbol| SymbolInfo {
            symbol: symbol.symbol().to_string(),
            status: symbol.status().to_string(),
            base_asset: symbol.base_asset().to_string(),
            quote_asset: symbol.quote_asset().to_string(),
            quote_asset_precision: symbol.quote_precision(),
            order_types: symbol.order_types().clone(),
            symbol_type: symbol.symbol_type().to_string(),
            on_board_time: symbol.get_on_board_time(),
        })
        .filter(|symbol| symbol.quote_asset == "USDT")
        .collect()
}

/// 获取现货交易对信息
pub async fn get_trading_spot_symbols(exchange: ExchangeInfo) -> Result<Vec<SymbolInfo>, YueError> {
    Ok(translate_symbols(&exchange.symbols))
}

pub const CONTRACT_TYPE_PERPETUAL: &str = "PERPETUAL";
// 默认从 2021-01-01 00:00:00 UTC 开始回补历史K线。
const DEFAULT_HISTORY_START_TIME_MS: u64 = 1609459200000;

/// 获取合约交易对信息
/// PERPETUAL 为永续
/// CURRENT_QUARTER：为下一季
/// NEXT_QUARTER：当前季度合约
pub async fn get_trading_swap_symbols(
    exchange: SwapExchangeInfo,
    status: Option<&str>,
    type_filter: Option<&str>,
) -> Result<Vec<SymbolInfo>, YueError> {
    let all = translate_symbols(exchange.symbols());
    if let Some(filter) = type_filter {
        Ok(all.into_iter().filter(|s| s.symbol_type == filter).collect())
    } else {
        Ok(all)
    }
}

pub type HistoryBatchHandler<O: HistoryVo + Clone + Send + Sync> = Box<dyn HistoryBatchHandlerTrait<O> + Send + Sync>;

///
///  因为有些需要批量的中间操作。所以这里就需要一个方法做为处理的类
///
#[async_trait]
pub trait HistoryBatchHandlerTrait<O>: Send + Sync
where
    O: HistoryVo + Clone + Send + Sync + 'static,
{
    async fn handle(&self, batch_data: Vec<O>) -> Result<(), YueError>;
}

///
/// 这样处理，最主要的目的是为了方便后续写单元测试。
/// 因为这类外部的IO类，rust下面很难写单元测试。所以也就这么搞了。
/// 主要是一种尝试。
///
pub type HistoryFetcher<O: HistoryVo + Clone + Send + Sync + 'static> = Box<dyn HistoryBatchHandlerTrait<O>>;

#[async_trait]
pub trait HistoryFetcherTrait<T, O>
where
    T: MuteHistoryParam + ToRequestBuilder + Send + Sync + 'static,
    O: HistoryVo + 'static,
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
        handler: Option<HistoryBatchHandler<O>>,
        retry_on_error: bool,
    ) -> Result<u64, YueError>;
}

#[derive(Debug, Clone)]
pub struct HistoryFetcherImpl {
    request_info: RequestInfo,
}

impl HistoryFetcherImpl {
    pub fn kline(request_info: &RequestInfo) -> Self {
        Self {
            request_info: request_info.clone(),
        }
    }
}

#[async_trait]
impl<T, O> HistoryFetcherTrait<T, O> for HistoryFetcherImpl
where
    T: MuteHistoryParam + ToRequestBuilder + Send + Sync + 'static,
    O: HistoryVo + 'static,
{
    /// 获取指定交易对和时间间隔的历史数据.
    /// 只是对参数做透传，而且只是定位最后的处理方式。不会做过多的其他操作。
    /// 外部自己处理特殊关系。
    ///
    /// 大致流程：
    /// 1. 判断当前的interval，如果为None的话，就是5分钟。
    /// 2. start_time为None的话，取2021年01月1日
    /// 3. end_time为None的话，取上一个周期的时间点。例如现在interval是5分钟，现在是36，那么end就是上一个35分的milliseconds减去1.
    /// 4. 循环调用，然后返回。
    ///     1. 每次循环，都把上一次的end_time作为下一次的start_time，end_time不变。保证每次请求的时间段是连续的。
    /// 5. 结束条件
    ///     1. 本次获得最后K线的日期和上一次相同。
    ///     2. 最后一条的end_time超过了或等于期望的end_time。因为有可能最后一条的end_time就是超过了期望的end_time的，所以需要判断一下。
    ///
    /// Option<HistoryBatchHandler<O>>
    /// 注意点
    /// 1. 最后一段时间最好废弃。比如说现在是11:30:00， interval是1h。那么最后一段就是11点到12点的一段时间。
    ///
    ///
    /// # 参数
    /// * `base_param` - 输入请求的基本参数，其应该包含symbol，limit，interval等信息。主要因为多次请求，会需要开始和结束时间。
    /// * `interval` - K线时间间隔，默认值为5分钟
    /// * `start_time` - 开始时间（毫秒时间戳），如果为None则获取全部历史数据，2021年01月01
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
        handler: Option<HistoryBatchHandler<O>>,
        retry_1000_times: bool,
    ) -> Result<u64, YueError> {
        let symbol = base_param.get_symbol();
        let mut error_count = 0;

        // 步骤1：interval 为 None 时默认使用 5 分钟。
        let chosen_interval = interval.unwrap_or(HistoryInterval::FiveMinutes);

        // 步骤2：start_time 为 None 时，从 2021-01-01 00:00:00 UTC 开始。
        let actual_start_time = start_time.unwrap_or(DEFAULT_HISTORY_START_TIME_MS);

        // 步骤3：end_time 为 None 时，取当前时间；并统一折算到“上一个完整周期”的末尾(-1ms)。
        let actual_end_time = end_time.unwrap_or(unix_time_now_u64_utc());

        let adjusted_start_time = Some(chosen_interval.get_close_unix_ms(actual_start_time));
        let adjusted_end_time = chosen_interval.get_close_unix_ms(actual_end_time).saturating_sub(ONE_MILL_SECOND_MS);

        // 入参时间窗口非法时直接返回，避免无意义请求。
        if adjusted_start_time.unwrap_or(0) > adjusted_end_time {
            debug!(
                "{} skip fetch because start {} > end {}",
                symbol,
                adjusted_start_time.unwrap_or(0),
                adjusted_end_time
            );
            return Ok(0);
        }

        debug!(
            "adjusted time range: {:?} to {} (using interval {})",
            adjusted_start_time,
            adjusted_end_time,
            chosen_interval.as_ref()
        );
        let mut total_count: u64 = 0;
        // 步骤4：根据 interval 连续翻页，直到满足注释里定义的退出条件。
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
            let params = base_param.create_new(current_start_time, Some(adjusted_end_time), Some(chosen_interval.clone()));

            let klines: Vec<O> = match execute_json_request::<Vec<O>>(&self.request_info, params.to_request_builder(&self.request_info), None).await {
                Ok(res) => res,
                Err(e) => {
                    error_count = error_count + 1;
                    if error_count > 1000 || !retry_1000_times {
                        error!(
                            "error symbol {} from {} when fetch data, error: {:?}",
                            symbol,
                            unix_2_readable(&current_start_time.unwrap()),
                            e
                        );
                        return Err(e);
                    }
                    continue;
                }
            };

            if klines.is_empty() {
                break;
            }

            let kline_num = klines.len();
            let klines_count = kline_num as u64;
            debug!("{} fetch {} kline", symbol, kline_num);
            // 直接读取最后一条 kline 的 close_time（u64 是 Copy），
            // 避免后续移动 `klines` 时出现借用跨越所有权边界。
            let last_close_time: Option<u64> = {
                // 把借用限制在小作用域，确保后面移动 `klines` 时没有活动借用。
                klines.last().map(|k| k.get_close_time())
            };

            if let Some(h) = &handler {
                if let Err(e) = h.handle(klines).await {
                    let jitter = Jitter::up_to(Duration::from_secs(1));
                    tokio::time::sleep(jitter + Duration::from_millis(10)).await;
                    warn!("batch handler error: {}", e);
                    continue;
                }
            }

            // 保留上一轮最后时间戳，用于判断本轮是否没有向前推进。
            let previous_last_timestamp = last_timestamp;
            if let Some(ts) = last_close_time {
                last_timestamp = Some(ts);
            }

            // 步骤5-1：本轮最后一条时间与上一轮相同（或倒退），说明翻页不再前进，直接退出且不计入本轮统计。
            if let (Some(prev), Some(curr)) = (previous_last_timestamp, last_timestamp) {
                if curr <= prev {
                    debug!("{} stop fetch because timestamp not forward: prev={}, curr={}", symbol, prev, curr);
                    break;
                }
            }

            // 不再需要对 `klines` 取引用来读取最后一条，
            // 上面已经把 close_time 读取并保存到 `last_close_time`/`last_timestamp`。
            total_count = total_count + klines_count;

            // 步骤5-2：最后一条已经到达(或超过)目标 end_time，结束。
            if let Some(ts) = last_timestamp {
                if ts >= adjusted_end_time {
                    break;
                }
            }

            // 步骤4：下一次请求从本次最后一条 close_time + 1ms 开始。
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
    use crate::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandler, HistoryFetcherImpl, HistoryFetcherTrait};
    use crate::errors::YueError;
    use crate::http_client::init_http_client;
    use crate::models::HistoryInterval;
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

    fn no_handler() -> Option<HistoryBatchHandler<BinanceKline>> {
        None
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
        let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_num: Result<u64, YueError> = fetcher
            .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), None, None, no_handler(), false)
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
        let third_start = base_open + (second_batch.len() as u64 + first_batch.len() as u64) * interval_ms;
        // end_time 设置在 1300h 处，保证两批数据都在窗口内
        // 生产代码: adjusted_end_time = get_close_unix_ms(end_time) - 1
        let end_time_input = base_open + 1300 * interval_ms;
        let adjusted_end_time = HistoryInterval::OneHour.get_close_unix_ms(end_time_input).saturating_sub(1);
        let empty_response: Vec<serde_json::Value> = vec![];

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

        // 第三次请求用于结束循环：返回空数组，触发 klines.is_empty() 退出。
        Mock::given(method("GET"))
            .and(path("/api/v3/klines"))
            .and(query_param("symbol", "BTCUSDT"))
            .and(query_param("interval", "1h"))
            .and(query_param("startTime", &third_start.to_string()))
            .and(query_param("endTime", &adjusted_end_time.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(
                base_param,
                Some(HistoryInterval::OneHour),
                Some(base_open),
                Some(end_time_input),
                no_handler(),
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
        let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), Some(1609459200000), None, no_handler(), false)
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
        // end_time 设置在 1200h 处，确保第一批 1000 条不会触发步骤5-2退出（还有余量）
        // 生产代码: adjusted_end_time = get_close_unix_ms(end_time) - 1
        let base_open = 1609459200000u64;
        let interval_ms = 3600000u64; // 1h
        let first_start = base_open;
        let second_start = base_open + (mock_klines.len() as u64) * interval_ms;
        let end_time_input = base_open + 1200 * interval_ms;
        let adjusted_end_time = HistoryInterval::OneHour.get_close_unix_ms(end_time_input).saturating_sub(1);

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
            .and(query_param("endTime", &adjusted_end_time.to_string()))
            .and(query_param("limit", "1000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(empty_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
        let base_param = CommonRequestBuilder::new("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let kline_res: Result<u64, YueError> = fetcher
            .get_all_kline_data(
                base_param,
                Some(HistoryInterval::OneHour),
                Some(base_open),
                Some(end_time_input),
                no_handler(),
                false,
            )
            .await;
        assert!(kline_res.is_ok(), "获取K线数据失败: {:?}", kline_res.as_ref().err());
        assert_eq!(kline_res.unwrap(), 1000);
    }
}

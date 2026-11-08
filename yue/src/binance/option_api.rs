use crate::binance::bn_models::option_restful::{KLinesResponse, MarketPriceResponse, OptionExchangeInfo};
use crate::binance::bn_restful_commands::{OPTION_EXCHANGE_COMMAND, OPTION_KLINE_COMMAND, OPTION_MARK_PRICE_COMMAND, execute_json_request};
use crate::binance::restful_func::CommonRequestBuilder;
use crate::errors::YueError;
use crate::http_client::{ToRequestBuilder, get_http_client};
use crate::models::HistoryInterval;
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait BinanceOptionApiTrait: Send + Sync {
    async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, YueError>;

    async fn get_kline_history(&self, request_builder: CommonRequestBuilder) -> Result<KLinesResponse, YueError>;

    async fn get_all_market_price(&self) -> Result<MarketPriceResponse, YueError>;

    ///
    /// 查询从start_time到end_time的所有K线数据。
    /// 1. 组装请求参数，limit为1500. 第一次调用，就用start_time和end_time查询，返回的K线数据可能不满1500条。
    /// 2. 如果返回超过1500条K线数据，则需要继续查询，start_time不变，end_time为上次最小的K线时间剪去1。
    /// 3. 如果返回为空或者返回的K线数据小于1500条，则说明已经查询完毕。返回结果
    ///
    async fn get_all_klines(&self, symbol: String, interval: String, start_time: u64, end_time: u64) -> Result<KLinesResponse, YueError> {
        get_all_klines_impl(self, symbol, interval, start_time, end_time).await
    }
}

async fn get_all_klines_impl<A: BinanceOptionApiTrait + ?Sized>(
    api: &A,
    symbol: String,
    interval: String,
    start_time: u64,
    end_time: u64,
) -> Result<KLinesResponse, YueError> {
    const LIMIT: u32 = 1500;

    if start_time > end_time {
        return Err(YueError::ParseError(format!(
            "start_time ({start_time}) must not be greater than end_time ({end_time})"
        )));
    }

    let interval = HistoryInterval::from_str(&interval)?;
    let mut current_end_time = end_time;
    let mut all_klines = BTreeMap::new();

    loop {
        if current_end_time < start_time {
            break;
        }

        let mut request_builder = CommonRequestBuilder::new(symbol.clone(), LIMIT, interval.clone());
        request_builder.start_time = Some(start_time);
        request_builder.end_time = Some(current_end_time);

        let page = api.get_kline_history(request_builder).await?;
        let page_len = page.len();
        if page_len == 0 {
            break;
        }

        let mut minimum_open_time = None;
        for kline in page {
            let open_time = kline.candle_begin_time()?;
            minimum_open_time = Some(minimum_open_time.map_or(open_time, |minimum: u64| minimum.min(open_time)));
            all_klines.insert(open_time, kline);
        }

        if page_len < LIMIT as usize {
            break;
        }

        let Some(next_end_time) = minimum_open_time.and_then(|time| time.checked_sub(1)) else {
            break;
        };
        if next_end_time < start_time || next_end_time >= current_end_time {
            break;
        }
        current_end_time = next_end_time;
    }

    Ok(all_klines.into_values().collect())
}

pub type BinanceOptionExchangeApi = Arc<dyn BinanceOptionApiTrait + Send + Sync>;

pub static SHARE_BN_OPTION_API: OnceLock<BinanceOptionExchangeApi> = OnceLock::new();
pub fn default_binance_option_api() -> BinanceOptionExchangeApi {
    SHARE_BN_OPTION_API.get_or_init(|| BinanceOptionExchangeApiImpl::new()).clone()
}

pub struct BinanceOptionExchangeApiImpl;

impl BinanceOptionExchangeApiImpl {
    pub fn new() -> BinanceOptionExchangeApi {
        Arc::new(BinanceOptionExchangeApiImpl {})
    }
}

#[async_trait]
impl BinanceOptionApiTrait for BinanceOptionExchangeApiImpl {
    async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(OPTION_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<OptionExchangeInfo>(&OPTION_EXCHANGE_COMMAND, rb, None).await
    }

    async fn get_kline_history(&self, request_builder: CommonRequestBuilder) -> Result<KLinesResponse, YueError> {
        execute_json_request::<KLinesResponse>(&OPTION_KLINE_COMMAND, request_builder.to_request_builder(&OPTION_KLINE_COMMAND), None).await
    }

    async fn get_all_market_price(&self) -> Result<MarketPriceResponse, YueError> {
        let client = get_http_client();
        let rb = client.get(OPTION_MARK_PRICE_COMMAND.as_ref().as_str());
        execute_json_request::<MarketPriceResponse>(&OPTION_MARK_PRICE_COMMAND, rb, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kline_response(open_times: impl IntoIterator<Item = u64>) -> KLinesResponse {
        let rows: Vec<_> = open_times.into_iter().map(|open_time| serde_json::json!([open_time])).collect();
        serde_json::from_value(serde_json::Value::Array(rows)).unwrap()
    }

    // 测试目的：验证 get_all_klines 按注释回退 end_time 并合并完整分页。
    // 准备数据/过程：mock 返回 1500 条满页及一页较早的不足量数据，调用 get_all_klines。
    // 预期结果：请求始终保留 start_time，后续 end_time 为前页最小 K 线时间减一，结果按时间升序且不丢页。
    #[tokio::test]
    async fn get_all_klines_paginates_backwards_until_partial_page() {
        let mut api = MockBinanceOptionApiTrait::new();
        let mut sequence = mockall::Sequence::new();
        api.expect_get_kline_history()
            .withf(|request| {
                request.symbol == "BTC-OPTION"
                    && request.interval == Some(HistoryInterval::OneMinute)
                    && request.limit == Some(1500)
                    && request.start_time == Some(0)
                    && request.end_time == Some(10_000)
            })
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(kline_response(1_000..2_500)));
        api.expect_get_kline_history()
            .withf(|request| request.start_time == Some(0) && request.end_time == Some(999))
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_| Ok(kline_response([0, 500, 999])));

        let klines = get_all_klines_impl(&api, "BTC-OPTION".to_string(), "1m".to_string(), 0, 10_000)
            .await
            .unwrap();

        assert_eq!(klines.len(), 1503);
        assert_eq!(klines.iter().next().unwrap().candle_begin_time().unwrap(), 0);
        assert_eq!(klines.iter().last().unwrap().candle_begin_time().unwrap(), 2499);
    }

    // 测试目的：验证 get_all_klines 对无效参数和底层请求错误进行显式返回。
    // 准备数据/过程：分别传入不支持的 interval、start_time 大于 end_time，以及返回请求错误的 mock API。
    // 预期结果：无效参数返回 ParseError，底层请求错误原样传播，参数错误时不发送请求。
    #[tokio::test]
    async fn get_all_klines_returns_interval_and_request_errors() {
        let invalid_interval_api = MockBinanceOptionApiTrait::new();
        assert!(matches!(
            get_all_klines_impl(&invalid_interval_api, "BTC-OPTION".to_string(), "2m".to_string(), 0, 10_000).await,
            Err(YueError::ParseError(_))
        ));

        let invalid_range_api = MockBinanceOptionApiTrait::new();
        assert!(matches!(
            get_all_klines_impl(&invalid_range_api, "BTC-OPTION".to_string(), "1m".to_string(), 10_000, 0).await,
            Err(YueError::ParseError(_))
        ));

        let mut request_error_api = MockBinanceOptionApiTrait::new();
        request_error_api
            .expect_get_kline_history()
            .times(1)
            .returning(|_| Err(YueError::RequestError("request failed".to_string())));
        assert!(matches!(
            get_all_klines_impl(&request_error_api, "BTC-OPTION".to_string(), "1m".to_string(), 0, 10_000).await,
            Err(YueError::RequestError(_))
        ));
    }

    // 测试目的：验证 get_all_klines 遇到空页时正常结束。
    // 准备数据/过程：mock 的首次历史 K 线请求返回空响应。
    // 预期结果：方法返回空 KLinesResponse，且只发送一次请求。
    #[tokio::test]
    async fn get_all_klines_stops_on_empty_page() {
        let mut api = MockBinanceOptionApiTrait::new();
        api.expect_get_kline_history().times(1).returning(|_| Ok(kline_response([])));

        let klines = get_all_klines_impl(&api, "BTC-OPTION".to_string(), "1m".to_string(), 0, 10_000)
            .await
            .unwrap();

        assert!(klines.is_empty());
    }
}

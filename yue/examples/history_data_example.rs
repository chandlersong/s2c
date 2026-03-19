use li::tools::logs::setup_logger_all;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{LevelFilter, debug, error, info};
use yue::binance::bn_models::common::HistoryVo;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::history_data::{CommonRequestBuilder, HistoryFetcher, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

fn print_kline_result<H>(result: &Result<(Vec<H>, u16), YueError>, interval: Option<HistoryInterval>)
where
    H: HistoryVo,
{
    match result {
        Ok((klines, fail_count)) => {
            debug!("Fetched {} data", klines.len());
            if let Some(first) = klines.first() {
                debug!("First kline open_time = {}", unix_2_readable(&first.get_open_time()));
            }
            if let Some(last) = klines.last() {
                debug!("Last kline close_time = {}", unix_2_readable(&last.get_close_time()));
            }
            debug!("total kline fetched: {}", klines.len());
            debug!("fail count {}", fail_count);

            // Interval consistency check: ensure each candle's begin (open_time) interval is equal
            if klines.len() >= 2 {
                // 计算相邻 open_time 差值数组
                let mut diffs: Vec<u64> = Vec::with_capacity(klines.len() - 1);
                for w in klines.windows(2) {
                    let prev = w[0].get_open_time();
                    let next = w[1].get_open_time();
                    let diff = if next >= prev { next - prev } else { 0 };
                    diffs.push(diff);
                }

                // 如果 caller 传入了 interval，就使用该 interval 的毫秒值作为期望间隔；
                // 否则回退为以第一个 diff 为期望间隔（向后兼容）
                let expected: u64 = if let Some(iv) = interval {
                    iv.to_milliseconds()
                } else if let Some(&first_diff) = diffs.first() {
                    first_diff
                } else {
                    0
                };

                if expected == 0 {
                    error!("Detected zero/unknown expected interval; cannot validate kline spacing");
                } else {
                    let mut ok = true;
                    for (i, &d) in diffs.iter().enumerate() {
                        if d != expected {
                            error!("Kline interval mismatch at window {}: expected {} ms, got {} ms", i, expected, d);
                            ok = false;
                        }
                    }
                    if ok {
                        debug!("Kline intervals consistent: {} ms", expected);
                    }
                }
            }
        }
        Err(e) => {
            error!("Error fetching klines: {:?}", e);
        }
    }
}

///
/// 这个例子，主要是是获取Kline，包括以下一些数据
/// 1. spot
/// 2. swap
/// 3. 资金费率
#[tokio::main]
async fn main() {
    // Initialize http client with default settings
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let _ = setup_logger_all(Some(LevelFilter::Debug));
    let now_ms = unix_time_now_u64_utc();
    let one_hour: u64 = 60 * 60 * 1000;
    let start_ms = now_ms - 10 * one_hour;
    let end_ms = now_ms - 10 * 1000 * 60;
    println!("Now (ms) = {}, start_time (ms) = {}", now_ms, start_ms);
    let symbol = "BTCUSDT";
    let spot_kline_fetch = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
    let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::FiveMinutes);
    let spot_btc: Result<(Vec<BinanceKline>, u16), YueError> = spot_kline_fetch
        .get_all_kline_data(base_param, Some(HistoryInterval::FiveMinutes), Some(start_ms), Some(end_ms))
        .await;
    info!("================fetch spot btc==============");
    print_kline_result(&spot_btc, Some(HistoryInterval::FiveMinutes));

    let swap_kline_fetch = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let swap_btc: Result<(Vec<BinanceKline>, u16), YueError> = swap_kline_fetch
        .get_all_kline_data(base_param, Some(HistoryInterval::OneHour), Some(start_ms), None)
        .await;
    info!("================fetch swap btc==============");
    print_kline_result(&swap_btc, Some(HistoryInterval::OneHour));

    // let swap_funding_rate_fetch = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    // let base_param = CommonParam::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    // let btc_funding_rate: Result<(Vec<FundingRate>, u16), YueError> =
    //     swap_funding_rate_fetch.get_all_kline_data(base_param, None, Some(start_ms), None).await;
    // info!("================fetch btc funding rate ==============");
    // print_kline_result(&btc_funding_rate, Some(HistoryInterval::OneHour));
}

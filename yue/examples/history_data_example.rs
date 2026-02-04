use li::tools::logs::setup_logger_all;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{LevelFilter, debug, error, info};
use yue::binance::bn_models::common::HistoryVo;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_FUNDING_RATE_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::history_data::{CommonParam, HistoryFetcher, HistoryInterval, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;

fn print_kline_result<H>(result: &Result<(Vec<H>, u16), YueError>)
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
    let start_ms = now_ms - 1500 * one_hour;
    println!("Now (ms) = {}, start_time (ms) = {}", now_ms, start_ms);
    let symbol = "BTCUSDT";
    let spot_kline_fetch = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
    let base_param = CommonParam::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let spot_btc: Result<(Vec<BinanceKline>, u16), YueError> = spot_kline_fetch.get_all_kline_data(base_param, Some(start_ms), None).await;
    info!("================fetch spot btc==============");
    print_kline_result(&spot_btc);

    let swap_kline_fetch = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let base_param = CommonParam::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let swap_btc: Result<(Vec<BinanceKline>, u16), YueError> = swap_kline_fetch.get_all_kline_data(base_param, Some(start_ms), None).await;
    info!("================fetch swap btc==============");
    print_kline_result(&swap_btc);

    let swap_funding_rate_fetch = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    let base_param = CommonParam::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let btc_funding_rate: Result<(Vec<FundingRate>, u16), YueError> =
        swap_funding_rate_fetch.get_all_kline_data(base_param, Some(start_ms), None).await;
    info!("================fetch btc funding rate ==============");
    print_kline_result(&btc_funding_rate);
}

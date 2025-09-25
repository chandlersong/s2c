use li::tools::logs::setup_logger_all;
use li::tools::time::{unix_2_readable, unix_time_now_u64};
use log::{LevelFilter, debug, error, info};
use yue::binance::bn_models::BinanceKline;
use yue::binance::bn_restful_commands::{SPOT_KLINE_COMMAND, SWAP_KLINE_COMMAND};
use yue::binance::history_data::{HistoryFetcher, HistoryInterval, KlineParams, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;

fn print_kline_result(result: &Result<(Vec<BinanceKline>, u16), YueError>, start_ms: u64, one_hour: u64) {
    match result {
        Ok((klines, fail_count)) => {
            debug!("Fetched {} klines", klines.len());
            if let Some(first) = klines.first() {
                debug!("First kline open_time = {}", unix_2_readable(&first.open_time));
            }
            if let Some(last) = klines.last() {
                debug!("Last kline close_time = {}", unix_2_readable(&last.close_time));
            }
            let mut prev = start_ms - one_hour;
            for k in klines {
                let gap = k.open_time - prev;
                if gap != one_hour {
                    debug!(
                        "Time gap detected!prev is {},now is {}",
                        unix_2_readable(&prev),
                        unix_2_readable(&k.open_time)
                    );
                }
                prev = k.open_time;
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
    let now_ms = unix_time_now_u64();
    let one_hour: u64 = 60 * 60 * 1000;
    let start_ms = now_ms - 1500 * one_hour;
    println!("Now (ms) = {}, start_time (ms) = {}", now_ms, start_ms);
    let symbol = "BTCUSDT";
    let spot_kline_fetch = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
    let base_param = KlineParams::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let spot_btc: Result<(Vec<BinanceKline>, u16), YueError> = spot_kline_fetch.get_all_kline_data(base_param, Some(start_ms)).await;
    info!("================fetch spot btc==============");
    print_kline_result(&spot_btc, start_ms, one_hour);

    let swap_kline_fetch = SimpleHistoryFetcher::new(&SWAP_KLINE_COMMAND);
    let base_param = KlineParams::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let swap_btc: Result<(Vec<BinanceKline>, u16), YueError> = swap_kline_fetch.get_all_kline_data(base_param, Some(start_ms)).await;
    info!("================fetch swap btc==============");
    print_kline_result(&swap_btc, start_ms, one_hour);
}

use li::tools::logs::{setup_logger, setup_logger_all};
use li::tools::time::{unix_2_readable, unix_time_now_u64};
use log::LevelFilter;
use yue::binance::bn_models::BinanceKline;
use yue::binance::history_data::{HistoryFetcher, HistoryInterval, KlineParams, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;

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
    let fetcher = SimpleHistoryFetcher {};
    let base_param = KlineParams::new(symbol.to_string(), 1000, HistoryInterval::OneHour);
    let res: Result<(Vec<BinanceKline>, u16), YueError> = fetcher.get_all_kline_data(base_param, Some(start_ms)).await;
    match res {
        Ok((klines, fail_count)) => {
            println!("Fetched {} klines", klines.len());
            if let Some(first) = klines.first() {
                println!("First kline open_time = {}", unix_2_readable(&first.open_time));
            }
            if let Some(last) = klines.last() {
                println!("Last kline close_time = {}", unix_2_readable(&last.close_time));
            }
            let mut prev = start_ms - one_hour;
            for k in &klines {
                let gap = k.open_time - prev;
                if gap != one_hour {
                    println!("Time gap detected!prev is {},now is {}", unix_2_readable(&prev), unix_2_readable(&k.open_time));
                }
                prev = k.open_time;
            }

            println!("total kline fetched: {}", klines.len());
            println!("fail count {}", fail_count)
        }
        Err(e) => {
            eprintln!("Error fetching klines: {:?}", e);
        }
    }
}

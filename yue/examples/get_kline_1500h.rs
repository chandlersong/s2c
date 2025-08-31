// Example: fetch last 1500 hours of 1h klines for BTCUSDT
use yue::binance::spots::{KlineInterval, get_all_kline_data};
use yue::http_client::init_http_client;
use yue::tools::{unix_2_readable, unix_time};

#[tokio::main]
async fn main() {
    // Initialize http client with default settings
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);

    let now_ms = unix_time();
    let hours: u64 = 1500;
    let start_ms = now_ms - hours * 3600 * 1000;
    println!("Now (ms) = {}, start_time (ms) = {}", now_ms, start_ms);

    match get_all_kline_data("BTCUSDT", KlineInterval::OneHour, Some(start_ms)).await {
        Ok(klines) => {
            println!("Fetched {} klines", klines.len());
            if let Some(first) = klines.first() {
                println!(
                    "First kline open_time = {}",
                    unix_2_readable(&first.open_time)
                );
            }
            if let Some(last) = klines.last() {
                println!(
                    "Last kline close_time = {}",
                    unix_2_readable(&last.close_time)
                );
            }
            println!("total kline fetched: {}", klines.len());
        }
        Err(e) => {
            eprintln!("Error fetching klines: {:?}", e);
        }
    }
}

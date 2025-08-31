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
    let one_hour: u64 = 60 * 60 * 1000;
    let start_ms = now_ms - 1500 * one_hour;
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
            let mut prev = start_ms - one_hour;
            for k in &klines {
                let gap = k.open_time - prev;
                if gap != one_hour {
                    println!(
                        "Time gap detected!prev is {},now is {}",
                        unix_2_readable(&prev),
                        unix_2_readable(&k.open_time)
                    );
                }
                prev = k.open_time;
            }

            println!("total kline fetched: {}", klines.len());
        }
        Err(e) => {
            eprintln!("Error fetching klines: {:?}", e);
        }
    }
}

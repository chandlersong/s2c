use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history_task::HistoryDataTask;
use yu::binance::models::po::KlinePo;
use yu::config::get_config;
use yu::errors::YuError;
use yu::exchange::{CloneHistoryFetcherFactory, HistoryFetcherFactory};
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::SWAP_KLINE_HISTORY_COMMAND;
use yue::binance::history_data::{CommonRequestBuilder, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }

    let mut special_log = HashMap::new();
    special_log.insert("mingluan".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();

    let base_swap_kline_fetcher = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);

    let param = CommonRequestBuilder::new("GRASSUSDT".to_string(), 1000, HistoryInterval::OneHour);
    let (tx, mut rx) = mpsc::channel::<Result<Vec<KlinePo>, YueError>>(100);
    let interval = HistoryInterval::FiveMinutes;
    let now_timestamp = unix_time_now_u64_utc();
    let start_time = interval.get_close_unix_ms(now_timestamp - 10 * 60 * 1000);
    let end_time = interval.get_close_unix_ms(now_timestamp);

    let fetch_handle = tokio::spawn(async move {
        HistoryDataTask::<
            CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline>,
            CommonRequestBuilder,
            KlinePo, // 修正为 KlinePo，满足 HistoryPO 约束
            BinanceKline,
            BinanceDashboard,
        >::fetch_symbol_data(swap_kline_fetcher.create_fetcher(), param, start_time, end_time, tx, "test", interval)
        .await;
    });

    use std::collections::HashSet;
    let mut all_open_times = Vec::new();
    let mut all_items = Vec::new();
    while let Some(result) = rx.recv().await {
        match result {
            Ok(data) => {
                for item in &data {
                    all_open_times.push(item.candle_begin_time);
                    all_items.push(item.clone());
                }
                let mut seen = HashSet::new();
                let mut duplicates = HashSet::new();
                for &ot in &all_open_times {
                    if !seen.insert(ot) {
                        duplicates.insert(ot);
                    }
                }
                if !duplicates.is_empty() {
                    println!("重复的open_time: {:?}", duplicates);
                    for ot in &duplicates {
                        for item in &all_items {
                            if item.candle_begin_time == *ot {
                                println!("重复项: {:?}", item);
                            }
                        }
                    }
                }
                // 新增：校验 candle_begin_time 是否严格相差一个小时
                if all_open_times.len() > 1 {
                    let mut last = all_open_times[0];
                    for (idx, &cur) in all_open_times.iter().enumerate().skip(1) {
                        if cur != last + 3600_000 {
                            println!(
                                "第{}项与前一项candle_begin_time间隔不是1小时: {} -> {} (差值: {} ms)",
                                idx,
                                last,
                                cur,
                                cur - last
                            );
                        }
                        last = cur;
                    }
                }
                if !all_open_times.is_empty() {
                    println!("open_time第一项: {:?}", all_open_times.first().unwrap());
                    println!("open_time最后一项: {:?}", all_open_times.last().unwrap());
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
    }
    let _ = fetch_handle.await;
    Ok(())
}

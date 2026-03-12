use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history_task::HistoryDataTask;
use yu::binance::models::po::FundingRatePo;
use yu::config::get_config;
use yu::errors::YuError;
use yu::exchange::{CloneHistoryFetcherFactory, HistoryFetcherFactory};
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::SWAP_FUNDING_RATE_COMMAND;
use yue::binance::history_data::{CommonParam, MuteHistoryParam, SimpleHistoryFetcher};
use yue::errors::YueError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

/// 建立这个例子，主要是在初始化的时候，发现GRASSUSDT一直取不到数据
/// 所以也就在这里用了一下
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

    let base_swap_funding_rate_fetcher = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    let swap_funding_rate_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, FundingRate> =
        CloneHistoryFetcherFactory::new(base_swap_funding_rate_fetcher);

    let param = CommonParam::initial("1000SHIBUSDT".to_string(), 1000, HistoryInterval::OneHour);
    let (tx, mut rx) = mpsc::channel::<Result<Vec<FundingRatePo>, YueError>>(100);

    let interval = HistoryInterval::FiveMinutes;
    let now_timestamp = unix_time_now_u64_utc();
    let start_time = interval.get_close_unix_ms(now_timestamp - 10 * 60 * 1000);
    let end_time = interval.get_close_unix_ms(now_timestamp);

    // 用tokio::spawn在后台异步任务中运行fetch_symbol_data
    let fetch_handle = tokio::spawn(async move {
        HistoryDataTask::<
            CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonParam, FundingRate>,
            CommonParam,
            FundingRatePo,
            FundingRate,
            BinanceDashboard,
        >::fetch_symbol_data(
            swap_funding_rate_fetcher.create_fetcher(),
            param,
            start_time,
            end_time,
            tx,
            "test",
            interval,
        )
        .await;
    });

    // 主线程异步接收数据
    use std::collections::HashSet;
    let mut all_funding_times = Vec::new();
    let mut all_items = Vec::new();
    while let Some(result) = rx.recv().await {
        match result {
            Ok(data) => {
                // 收集所有funding_time和原始item
                for item in &data {
                    all_funding_times.push(item.funding_time);
                    all_items.push(item.clone());
                }
                // 统计重复的funding_time，并打印所有重复行
                let mut seen = HashSet::new();
                let mut duplicates = HashSet::new();
                for &ft in &all_funding_times {
                    if !seen.insert(ft) {
                        duplicates.insert(ft);
                    }
                }
                if !duplicates.is_empty() {
                    println!("重复的funding_time: {:?}", duplicates);
                    for ft in &duplicates {
                        for item in &all_items {
                            if item.funding_time == *ft {
                                println!("重复项: {:?}", item);
                            }
                        }
                    }
                }
                if !all_funding_times.is_empty() {
                    println!("funding_time第一项: {:?}", all_funding_times.first().unwrap());
                    println!("funding_time最后一项: {:?}", all_funding_times.last().unwrap());
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
    }
    // 等待后台任务完成
    let _ = fetch_handle.await;
    Ok(())
}

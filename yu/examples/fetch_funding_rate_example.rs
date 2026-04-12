use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history_task::HistoryDataTask;
use yu::config::get_config;
use yu::errors::YuError;
use yu::exchange::{CloneHistoryFetcherFactory, HistoryFetcherFactory};
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::SWAP_FUNDING_RATE_COMMAND;
use yue::binance::history_data::{CommonRequestBuilder, SimpleHistoryFetcher};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::query_message::QueryCommand;

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

    let base_swap_funding_rate_fetcher = SimpleHistoryFetcher::kline(&SWAP_FUNDING_RATE_COMMAND);
    let swap_funding_rate_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, FundingRate> =
        CloneHistoryFetcherFactory::new(base_swap_funding_rate_fetcher);

    let param = CommonRequestBuilder::new("1000SHIBUSDT".to_string(), 1000, HistoryInterval::OneHour);

    let interval = HistoryInterval::FiveMinutes;
    let now_timestamp = unix_time_now_u64_utc();
    let start_time = interval.get_close_unix_ms(now_timestamp - 10 * 60 * 1000);
    let end_time = interval.get_close_unix_ms(now_timestamp);
    // 用tokio::spawn在后台异步任务中运行fetch_symbol_data
    let (swap_recipient, mut swap_rx) = mpsc::channel::<QueryCommand<FundingRate>>(100);
    let fetch_handle = tokio::spawn(async move {
        HistoryDataTask::<
            CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, FundingRate>,
            CommonRequestBuilder,
            FundingRate,
            BinanceDashboard,
        >::fetch_symbol_data(
            swap_funding_rate_fetcher.create_fetcher(),
            param,
            start_time,
            end_time,
            "test",
            interval,
            swap_recipient,
        )
        .await;
    });
    // 等待后台任务完成
    let _ = fetch_handle.await;
    while let Some(msg) = swap_rx.recv().await {
        match msg {
            QueryCommand::GetCount(_) => {
                info!("receive GetCount");
            }
            QueryCommand::BatchInsert(payload) => {
                info!("receive BatchInsertPayload");
                for d in payload.data.iter() {
                    println!("{:?}", d);
                }
                if let Some(callback) = payload.callback {
                    callback.send(Ok(1)).unwrap();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

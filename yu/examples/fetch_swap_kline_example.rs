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
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::SWAP_KLINE_HISTORY_COMMAND;
use yue::binance::restful_func::{CommonRequestBuilder, SimpleHistoryFetcher};
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::query_message::QueryCommand;

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

    let base_swap_kline_fetcher = SimpleHistoryFetcher::kline(&SWAP_KLINE_HISTORY_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);

    let param = CommonRequestBuilder::new("GRASSUSDT".to_string(), 1000, HistoryInterval::OneHour);
    let interval = HistoryInterval::FiveMinutes;
    let now_timestamp = unix_time_now_u64_utc();
    let start_time = interval.get_close_unix_ms(now_timestamp - 10 * 60 * 1000);
    let end_time = interval.get_close_unix_ms(now_timestamp);

    let (swap_recipient, mut swap_rx) = mpsc::channel::<QueryCommand<BinanceKline>>(100);
    let _ = tokio::spawn(async move {
        HistoryDataTask::<
            CloneHistoryFetcherFactory<SimpleHistoryFetcher, CommonRequestBuilder, BinanceKline>,
            CommonRequestBuilder,
            BinanceKline,
            BinanceDashboard,
        >::fetch_symbol_data(
            swap_kline_fetcher.create_fetcher(),
            param,
            start_time,
            end_time,
            "test",
            interval,
            swap_recipient,
        )
        .await;
    });
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

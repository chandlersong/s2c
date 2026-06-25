use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::service::SeriesHistoryMarketService;
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
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    special_log.insert("series_history_market_service_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();

    let series_ids = vec!["45".to_string(), "10151".to_string(), "10041".to_string()];

    let service = SeriesHistoryMarketService::new(series_ids, HistoryInterval::OneHour).await;
    Ok(())
}

use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::database::initial_polymarket_tables;
use yu::polymarket::service::default_series_history_market_service;
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

    initial_polymarket_tables(None)?;
    // let series_ids = vec!["45".to_string(), "10151".to_string(), "10041".to_string()];
    let series_ids = vec!["45".to_string()];
    let service = default_series_history_market_service(series_ids, HistoryInterval::OneHour).await;
    service.sync_instrument().await?;
    let instruments = service.list_instruments().await?;
    info!("find instruments num: {}", instruments.len());
    let mut rx = service.subscribe_history_broadcast();
    let check_service = service.clone();
    tokio::spawn(async move {
        service.fetch_latest_history().await.expect("TODO: panic message");
    });
    tokio::spawn(async move {
        while let Ok(history) = rx.recv().await {
            info!("receive history: {}", history);
        }
    });
    info!("start check history data");
    if let Err(e) = check_service.check_history_data().await {
        error!("check history data error: {}", e);
    }
    tokio::time::sleep(tokio::time::Duration::from_mins(5)).await;
    Ok(())
}

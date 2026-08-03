use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::database::initial_polymarket_tables;
use yu::polymarket::service::default_series_history_market_service;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::default_polymarket_api;

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
    let service = default_series_history_market_service(series_ids, HistoryInterval::OneHour, default_polymarket_api(), None).await;
    service.sync_instrument().await?;
    let instruments = service.list_instruments().await?;
    info!("find instruments num: {}", instruments.len());
    // tokio::spawn(async move {
    //     service.fetch_last_round_data().await.expect("TODO: panic message");
    // });

    Ok(())
}

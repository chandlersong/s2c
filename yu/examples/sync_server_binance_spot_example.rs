use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::sync_job::start_binance_sync_server_job;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("yu".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    special_log.insert("sync_server_binance_spot_example".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let app_config = get_config();
    init_http_client(app_config.proxy_url.as_deref());

    let (history_tx, mut history_rx) = mpsc::channel::<HistoryEntry>(1000);
    let history_receiver = tokio::spawn(async move {
        while let Some(entry) = history_rx.recv().await {
            info!("received Binance Spot history entry: {:?}", entry);
        }
    });

    let instrument_service = start_binance_sync_server_job(&app_config, history_tx).await?;
    let instruments = instrument_service.list_instruments().await?;
    info!("Binance Spot instruments in database: {}", instruments.len());

    let history_services = instrument_service.support_history_list()?;
    let history_service = history_services
        .get(&HistoryType::BinanceSpot)
        .ok_or_else(|| YuError::ConfigError("Binance Spot history service is not registered".to_string()))?;
    history_service.check_history_data().await?;
    history_service.sync_all().await?;

    history_receiver
        .await
        .map_err(|error| YuError::new(&format!("history receiver task failed: {error}")))?;
    Ok(())
}

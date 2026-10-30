use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::db_consts::initial_tables;
use yu::binance::sync_job::start_binance_sync_server_job;
use yu::binance::sync_server::{BinanceSwapFundingRateSyncHistoryService, BinanceSyncInstrumentService};
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use yu::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("sync_server_binance_swap_funding_rate_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let app_config = get_config();
    if let Some(proxy_url) = &app_config.proxy_url {
        init_http_client(Some(proxy_url));
    } else {
        init_http_client(None);
    }

    let (history_tx, mut history_rx) = mpsc::channel::<HistoryEntry>(1000);
    tokio::spawn(async move {
        while let Some(entry) = history_rx.recv().await {
            info!("received Binance Swap Funding Rate history entry: {:?}", entry);
        }
    });

    if std::env::args().any(|arg| arg == "--job") {
        start_binance_sync_server_job(&app_config, history_tx).await?;
        info!("Binance Spot, Swap, and Funding Rate sync jobs started; press Ctrl+C to stop");
        tokio::signal::ctrl_c()
            .await
            .map_err(|error| YuError::new(&format!("failed to wait for Ctrl+C: {error}")))?;
        return Ok(());
    }

    initial_tables(None)?;
    let dashboard = Arc::new(BinanceDashboard::new(app_config.get_data_retention_hours(), None));
    let history_service = BinanceSwapFundingRateSyncHistoryService::for_production(dashboard.clone(), app_config.get_data_retention_ms(), history_tx);
    let history_services: HashMap<HistoryType, SyncHistoryService> =
        HashMap::from([(HistoryType::BinanceSwapFundingRate, history_service.clone() as SyncHistoryService)]);
    let instrument_service = BinanceSyncInstrumentService::for_production(dashboard, history_services);

    if let Err(error) = instrument_service.sync_instrument().await {
        error!("failed to sync Binance instruments: {error}");
        return Err(error);
    }
    let instruments = instrument_service.list_instruments().await?;
    info!("Binance instruments in database: {}", instruments.len());

    history_service.initial_history_data(None).await?;
    history_service.check_history_data().await?;
    history_service.sync_all().await?;
    info!("Binance Swap Funding Rate sync service started; press Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| YuError::new(&format!("failed to wait for Ctrl+C: {error}")))?;
    Ok(())
}

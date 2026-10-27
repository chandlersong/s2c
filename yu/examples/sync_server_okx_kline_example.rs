use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::duck_po::InstrumentPo;
use yu::okx::duckdb_repository::get_default_kline_repo;
use yu::okx::service::OptionService;
use yu::okx::sync_server::{OkxSyncHistoryService, OkxSyncInstrumentService, OkxSyncKlineHandler};
use yu::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use yu::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("sync_server_okx_kline_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(proxy_url) = proxy {
        init_http_client(Some(&proxy_url));
    } else {
        init_http_client(None);
    }

    let (history_tx, mut history_rx) = mpsc::channel::<HistoryEntry>(1000);
    let history_receiver = tokio::spawn(async move {
        while let Some(entry) = history_rx.recv().await {
            info!("received OKX history entry: {:?}", entry);
        }
    });
    let live_instruments = Arc::new(RwLock::new(Vec::<InstrumentPo>::new()));
    let okx_option_service = OptionService::for_production(live_instruments.clone());

    let kline_history_service = OkxSyncHistoryService::for_production(history_tx.clone(), okx_option_service.clone())?;

    let mut support_history_service: HashMap<HistoryType, SyncHistoryService> = HashMap::new();
    support_history_service.insert(HistoryType::OkxKline, kline_history_service.clone());
    let instrument_service = OkxSyncInstrumentService::for_production(support_history_service, live_instruments.clone());
    if let Err(e) = instrument_service.sync_instrument().await {
        error!("error when syncing instrument: {}", e);
    }

    let instruments_in_db = instrument_service.list_instruments().await?;

    info!("OKX instruments in database num: {}", instruments_in_db.len());
    let handler = Arc::new(OkxSyncKlineHandler::new(
        get_default_kline_repo(None),
        history_tx.clone(),
        live_instruments.clone(),
    ));
    if let Err(e) = okx_option_service.start(handler).await {
        error!("failed to start OKX option listen okx kline: {:?}", e);
        return Err(YuError::from(e));
    }
    if let Err(e) = kline_history_service.initial_history_data(None).await {
        error!("failed to initial OKX kline history data: {:?}", e);
        return Err(e);
    }
    if let Err(error) = kline_history_service.check_history_data().await {
        error!("failed to check OKX kline history: {}", error);
    }
    drop(instrument_service);
    history_receiver
        .await
        .map_err(|error| YuError::new(&format!("history receiver task failed: {error}")))?;
    Ok(())
}

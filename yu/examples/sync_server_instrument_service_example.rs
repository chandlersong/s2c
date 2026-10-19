use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::database::initial_polymarket_tables;
use yu::polymarket::duckdb_repository::get_instrument_repo;
use yu::polymarket::po::PolyMarketInstrumentPo;
use yu::polymarket::sync_server::{PolyMarketSyncHistoryService, PolyMarketSyncInstrumentService};
use yu::sync::models::grpc_sync::instrument::Payload::Polymarket;
use yu::sync::models::grpc_sync::{HistoryEntry, InstrumentType};
use yu::sync::server::sync_server::{SyncHistoryService, SyncInstrumentServiceTrait};
use yue::http_client::init_http_client;
use yue::polymarket::restful_api::default_polymarket_api;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("sync_server_instrument_service_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    if let Err(e) = initial_polymarket_tables(None) {
        error!("error when initial polymarket table:{}", e)
    }
    let live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>> = Arc::new(RwLock::new(Vec::new()));
    let (history_tx, history_rx) = mpsc::channel::<HistoryEntry>(10000);
    let history_receiver = std::thread::spawn(move || {
        let mut history_rx = history_rx;
        while let Some(message) = history_rx.blocking_recv() {
            info!("received history message: {:?}", message);
        }
    });
    let token_history = PolyMarketSyncHistoryService::for_production(live_instruments.clone(), history_tx);

    let mut polymarket_sync_history_service_map: HashMap<InstrumentType, SyncHistoryService> = HashMap::new();
    polymarket_sync_history_service_map.insert(InstrumentType::PolymarketToken, token_history.clone());
    let series_ids = vec!["45".to_string()];

    let instrument_service = PolyMarketSyncInstrumentService::for_production(series_ids, polymarket_sync_history_service_map, live_instruments);
    if let Err(e) = instrument_service.sync_instrument().await {
        error!("error when sync instrument,error is {:?}", e);
    }
    let instruments = instrument_service.list_instruments().await?;

    for (key, inst) in instruments {
        if let Some(Polymarket(poly)) = inst.payload {
            info!("{}: {}", key, poly.asset_slug);
        }
    }
    // if let Err(e) = token_history.initial_history_data().await {
    //     error!("error initial history data: {}", e)
    // }
    // if let Err(e) = token_history.check_history_data().await {
    //     error!("error checking history_data: {}", e)
    // }
    if let Err(e) = token_history.fetch_latest_history().await {
        error!("error fetch latest history: {}", e)
    }
    drop(instrument_service);
    drop(token_history);
    history_receiver.join().map_err(|_| YuError::new("history receiver thread panicked"))?;
    Ok(())
}

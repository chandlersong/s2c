use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use yu::config::get_config;
use yu::errors::YuError;
use yu::polymarket::database::initial_polymarket_tables;
use yu::polymarket::duckdb_repository::get_instrument_repo;
use yu::polymarket::sync_job::start_polymarket_sync_series_job;
use yu::polymarket::sync_server::{PolyMarketSyncHistoryService, PolyMarketSyncInstrumentService};
use yu::sync::models::grpc_sync::InstrumentType;
use yu::sync::models::grpc_sync::instrument::Payload::Polymarket;
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

    let history_service = start_polymarket_sync_series_job(&app_config).await?;
    let polymarket_sync_history_service_map = HashMap::new();
    // polymarket_sync_history_service_map.insert(
    //     InstrumentType::PolymarketToken,
    //     PolyMarketSyncHistoryService::new(history_service.clone()),
    // );
    let client = default_polymarket_api();
    let inst_repo = get_instrument_repo(None);
    let series_ids = vec!["45".to_string()];

    let instrument_service = PolyMarketSyncInstrumentService::new(series_ids, polymarket_sync_history_service_map, client, inst_repo);

    let instruments = instrument_service.list_instruments().await?;

    for (key, inst) in instruments {
        if let Some(Polymarket(poly)) = inst.payload {
            info!("{}: {}", key, poly.asset_slug);
        }
    }
    Ok(())
}

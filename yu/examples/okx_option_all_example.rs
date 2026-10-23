use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::duck_po::{InstrumentPo, OkxKlinePo};
use yu::okx::duckdb_repository::{get_default_kline_repo, get_instrument_repo};
use yu::okx::duckdb_tables::initial_okx_tables;
use yu::okx::service::{KlineHandler, OptionService};
use yue::http_client::init_http_client;
use yue::models::InstrumentType;

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
    special_log.insert("okx_option_all_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    initial_okx_tables(None)?;
    let kline_repo = get_default_kline_repo(None);
    let instrument_repo = get_instrument_repo(None);
    let live_instruments = instrument_repo.get_instrument_by_type_live(InstrumentType::Option).await?;
    let service = OptionService::for_production(Arc::new(RwLock::new(live_instruments.clone())));

    let id_identify_dict = live_instruments
        .clone()
        .into_iter()
        .map(|inst| (inst.inst_identify.clone(), inst))
        .collect::<HashMap<String, InstrumentPo>>();
    let (tx, _) = tokio::sync::broadcast::channel::<OkxKlinePo>(1000);
    let handler = KlineHandler::new(kline_repo, instrument_repo, id_identify_dict, tx.clone());
    service.start(Arc::new(handler)).await?;
    let sync_inst_service = service.clone();

    tokio::spawn(async move {
        let _ = sync_inst_service.initial_candle(0, None).await;
    });
    if let Err(e) = service.query_option_summary().await {
        info!("sync instrument error: {}", e);
    }

    sleep(Duration::from_mins(10)).await;
    Ok(())
}

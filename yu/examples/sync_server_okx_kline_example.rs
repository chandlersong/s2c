use li::tools::logs::setup_logger;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::sync_job::start_okx_sync_server_job;
use yu::sync::models::grpc_sync::HistoryEntry;
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

    let sync_service = start_okx_sync_server_job(&app_config, history_tx).await?;
    if let Err(e) = sync_service.sync_instrument().await {
        error!("error when syncing instrument: {}", e);
    }
    // let history_service = sync_service
    //     .support_history_list()?
    //     .remove(&HistoryType::OkxKline)
    //     .ok_or_else(|| YuError::new("OKX Kline history service is not registered"))?;
    // history_service.initial_history_data().await?;
    // history_service.check_history_data().await?;
    // history_service.sync_all().await?;

    let instruments_in_db = sync_service.list_instruments().await?;

    info!("OKX instruments in database num: {}", instruments_in_db.len());

    drop(sync_service);
    history_receiver
        .await
        .map_err(|error| YuError::new(&format!("history receiver task failed: {error}")))?;
    Ok(())
}

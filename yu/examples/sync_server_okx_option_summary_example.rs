use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::sync_job::start_okx_sync_server_job;
use yu::sync::models::grpc_sync::{HistoryEntry, HistoryType, ServerMessage};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("sync_server_okx_option_summary_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Info), special_log)?;

    let app_config = get_config();
    if let Some(proxy_url) = app_config.proxy_url.as_deref() {
        init_http_client(Some(proxy_url));
    } else {
        init_http_client(None);
    }

    let (history_tx, mut history_rx) = mpsc::channel::<HistoryEntry>(1000);
    let history_receiver = tokio::spawn(async move {
        while let Some(entry) = history_rx.recv().await {
            info!("received OKX history entry: {:?}", entry);
        }
    });

    let instrument_service = start_okx_sync_server_job(&app_config, history_tx).await?;
    let instruments = instrument_service.list_instruments().await?;
    let mut supported_history = instrument_service.support_history_list()?;
    let history_service = supported_history
        .remove(&HistoryType::OkxOptionSummary)
        .ok_or_else(|| YuError::new("OKX option summary history service is not registered"))?;

    history_service.initial_history_data(None).await?;
    history_service.sync_all().await?;

    let (query_tx, mut query_rx) = mpsc::channel::<Result<ServerMessage, tonic::Status>>(10);
    let query_receiver = tokio::spawn(async move {
        while let Some(message) = query_rx.recv().await {
            info!("received queried OKX option summary batch: {:?}", message);
        }
    });
    if let Some(inst_id) = instruments.iter().find_map(|instrument| match instrument.payload.as_ref() {
        Some(yu::sync::models::grpc_sync::instrument::Payload::Okx(okx)) => Some(okx.server_id),
        _ => None,
    }) {
        history_service
            .query_and_broadcast_history(inst_id, 0, li::tools::time::unix_time_now_u64_utc(), 100, query_tx)
            .await?;
    }

    query_receiver
        .await
        .map_err(|error| YuError::new(&format!("query receiver task failed: {error}")))?;
    history_receiver
        .await
        .map_err(|error| YuError::new(&format!("history receiver task failed: {error}")))?;
    Ok(())
}

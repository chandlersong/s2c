use crate::config::AppConfig;
use crate::cron_job;
use crate::errors::YuError;
use crate::okx::duck_po::InstrumentPo;
use crate::okx::duckdb_repository::get_instrument_repo;
use crate::okx::duckdb_tables::initial_okx_tables;
use crate::okx::service::OptionService;
use crate::okx::sync_server::{OkxSyncHistoryService, OkxSyncInstrumentService};
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService};
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

pub async fn start_okx_sync_server_job(app_config: &AppConfig, history_tx: mpsc::Sender<HistoryEntry>) -> Result<SyncInstrumentService, YuError> {
    let okx_config = app_config
        .okx
        .as_ref()
        .ok_or_else(|| YuError::ConfigError("OKX sync server is not configured".to_string()))?;
    if !okx_config.enable() {
        return Err(YuError::ConfigError("OKX sync server is disabled".to_string()));
    }

    initial_okx_tables(None)?;

    let option_service = Arc::new(OptionService::default());
    let live_instruments = Arc::new(RwLock::new(Vec::<InstrumentPo>::new()));
    let history_service = OkxSyncHistoryService::for_production(option_service.clone(), history_tx);
    history_service.initial_history_data().await?;

    let check_history_job = history_service.clone();
    let _ = cron_job!("0 17 */6 * * *", move |_uuid, _locked| {
        let each_sync = check_history_job.clone();
        Box::pin(async move {
            debug!("start checking OKX kline history");
            if let Err(error) = each_sync.check_history_data().await {
                error!("failed to check OKX kline history: {}", error);
            }
            debug!("finished checking OKX kline history");
        })
    });

    let sync_history_job = history_service.clone();
    let _ = cron_job!("0 0 * * * *", move |_uuid, _locked| {
        let each_sync = sync_history_job.clone();
        Box::pin(async move {
            debug!("start syncing latest OKX kline history");
            if let Err(error) = each_sync.sync_all().await {
                error!("failed to sync latest OKX kline history: {}", error);
            }
            debug!("finished syncing latest OKX kline history");
        })
    });

    let mut support_history_service: HashMap<HistoryType, SyncHistoryService> = HashMap::new();
    support_history_service.insert(HistoryType::OkxKline, history_service);

    let instrument_repo = get_instrument_repo(None);
    let instrument_service = OkxSyncInstrumentService::for_production(support_history_service, live_instruments);
    Ok(instrument_service)
}

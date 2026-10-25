use crate::config::AppConfig;
use crate::cron_job;
use crate::errors::YuError;
use crate::okx::duck_po::InstrumentPo;
use crate::okx::duckdb_repository::{get_default_kline_repo, get_default_option_summary_repo};
use crate::okx::duckdb_tables::initial_okx_tables;
use crate::okx::service::OptionService;
use crate::okx::sync_server::{OkxSyncHistoryService, OkxSyncInstrumentService, OkxSyncKlineHandler, OkxSyncOptionSummaryHistoryService};
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use log::{debug, error};
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
    let live_instruments = Arc::new(RwLock::new(Vec::<InstrumentPo>::new()));
    let okx_option_service = OptionService::for_production(live_instruments.clone());

    let history_service = OkxSyncHistoryService::for_production(history_tx.clone(), okx_option_service.clone())?;
    let option_summary_history_service =
        OkxSyncOptionSummaryHistoryService::for_production(history_tx.clone(), okx_option_service.clone(), get_default_option_summary_repo(None));

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

    let mut support_history_service: HashMap<HistoryType, SyncHistoryService> = HashMap::new();
    support_history_service.insert(HistoryType::OkxKline, history_service.clone());
    support_history_service.insert(HistoryType::OkxOptionSummary, option_summary_history_service.clone());

    let instrument_service = OkxSyncInstrumentService::for_production(support_history_service, live_instruments.clone());

    let sync_instrument_job = instrument_service.clone();
    if let Err(error) = sync_instrument_job.sync_instrument().await {
        error!("failed to sync okx option instrument when server start: {}", error);
    }
    let instruments = instrument_service.list_instruments().await?;
    debug!("loaded {} OKX instruments for sync server", instruments.len());
    let _ = cron_job!("0 17 */6 * * *", move |_uuid, _locked| {
        let each_sync = sync_instrument_job.clone();
        Box::pin(async move {
            debug!("start syncing okx option instrument");
            if let Err(error) = each_sync.sync_instrument().await {
                error!("failed to sync okx option instrument: {}", error);
            }
            debug!("finished syncing okx option instrument");
        })
    });
    let handler = Arc::new(OkxSyncKlineHandler::new(
        get_default_kline_repo(None),
        history_tx.clone(),
        live_instruments.clone(),
    ));
    if let Err(e) = okx_option_service.start(handler).await {
        error!("failed to start OKX option listen okx kline: {:?}", e);
        return Err(YuError::from(e));
    }
    if let Err(e) = history_service.initial_history_data().await {
        error!("failed to initial OKX kline history data: {:?}", e);
        return Err(e);
    }
    if let Err(e) = option_summary_history_service.initial_history_data().await {
        error!("failed to initial OKX option summary history data: {:?}", e);
        return Err(e);
    }

    let sync_option_summary_job = option_summary_history_service.clone();
    let _ = cron_job!("0 1 * * * *", move |_uuid, _locked| {
        let each_sync = sync_option_summary_job.clone();
        Box::pin(async move {
            debug!("start syncing OKX option summary");
            if let Err(error) = each_sync.sync_all().await {
                error!("failed to sync OKX option summary: {}", error);
            }
            debug!("finished syncing OKX option summary");
        })
    });
    Ok(instrument_service)
}

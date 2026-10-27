use crate::binance::bn_backend_service::get_spot_kline_table;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::db_consts::{drop_tables, initial_tables};
use crate::binance::sync_server::{BinanceSpotSyncHistoryService, BinanceSyncSpotInstrumentService};
use crate::binance::websocket_service::KlineSubscribeService;
use crate::config::AppConfig;
use crate::cron_job;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use yue::models::HistoryInterval;

pub async fn start_binance_sync_server_job(app_config: &AppConfig, history_tx: mpsc::Sender<HistoryEntry>) -> Result<SyncInstrumentService, YuError> {
    if !app_config.binance.as_ref().is_some_and(|config| config.enable()) {
        return Err(YuError::ConfigError("Binance sync server is not configured or disabled".to_string()));
    }
    drop_tables(None)?;
    initial_tables(None)?;
    info!(
        "Binance sync server job started with data retention hours: {}",
        app_config.get_data_retention_hours()
    );

    let interval = HistoryInterval::OneMinute;

    let dash_board = Arc::new(BinanceDashboard::new(app_config.get_data_retention_hours(), None));

    let history_service = BinanceSpotSyncHistoryService::for_production(
        app_config.get_data_retention_ms(),
        history_tx.clone(),
        dash_board.clone(),
        interval.clone(),
    );
    let mut support_history_service: HashMap<HistoryType, SyncHistoryService> = HashMap::new();
    support_history_service.insert(HistoryType::BinanceSpot, history_service.clone());
    let instrument_service = BinanceSyncSpotInstrumentService::for_production(dash_board.clone(), support_history_service);
    info!("start initial Binance Spot instruments");
    if let Err(error) = instrument_service.sync_instrument().await {
        error!("failed to sync Binance Spot instruments: {}", error);
    }
    info!("finished initial Binance Spot instruments data");

    let check_history_job = history_service.clone();
    let _ = cron_job!("0 23 */6 * * *", move |_uuid, _locked| {
        let each_sync = check_history_job.clone();
        Box::pin(async move {
            debug!("start checking Binance Spot kline history");
            if let Err(error) = each_sync.check_history_data().await {
                error!("failed to check Binance Spot kline history: {}", error);
            }
            debug!("finished checking Binance Spot kline history");
        })
    });

    let sync_instrument_job = instrument_service.clone();
    let _ = cron_job!("0 17 */6 * * *", move |_uuid, _locked| {
        let each_sync = sync_instrument_job.clone();
        Box::pin(async move {
            debug!("start syncing Binance Spot instruments");
            if let Err(error) = each_sync.sync_instrument().await {
                error!("failed to sync Binance Spot instruments: {}", error);
            } else {
                debug!("finished syncing Binance Spot instruments");
            }
        })
    });

    //
    // 先监听，在初始化，是为了防止丢失数据。
    //
    KlineSubscribeService::startup_spot_sync(
        get_spot_kline_table(),
        dash_board,
        app_config.proxy_url.clone(),
        interval,
        history_tx,
        Arc::new(RwLock::new(HashMap::new())),
    )
    .await?;
    info!("started initial Binance Spot kline history data");
    history_service.initial_history_data().await?;
    info!("finished initial Binance Spot kline history data");
    history_service.check_history_data().await?;

    Ok(instrument_service)
}

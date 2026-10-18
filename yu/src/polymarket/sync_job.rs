use crate::config::AppConfig;
use crate::cron_job;
use crate::errors::YuError;
use crate::polymarket::service::{SeriesHistoryMarketService, default_series_history_market_service};
use crate::polymarket::sync_server::{PolyMarketSyncHistoryService, PolyMarketSyncInstrumentService};
use crate::sync::models::grpc_sync::InstrumentType;
use crate::sync::server::sync_server::SyncInstrumentService;
use log::{error, info};
use std::collections::HashMap;
use yue::models::HistoryInterval;

///
/// 按照 series的级别去同步数据
/// 1. 初始化现有的数据。
///     - 获取最新的instrument
///     - 获取最新的history.
/// 2. 开启监听的进程。
///     - History的最新
///     - instrument的更新
///
pub async fn start_polymarket_sync_series_job(app_config: &AppConfig) -> Result<SyncInstrumentService, YuError> {
    let sync_server_config = app_config.sync_server.clone();
    if sync_server_config.is_none() {
        return Err(YuError::ConfigError("no sync serve configuration".to_string()));
    }
    let polymarket_config = match &app_config.polymarket {
        None => {
            return Err(YuError::ConfigError("polymarket not config".to_string()));
        }
        Some(c) => c,
    };
    let series_ids = polymarket_config.series_ids.clone();
    if series_ids.is_none() {
        return Err(YuError::ConfigError("no series id configuration".to_string()));
    }
    let service = default_series_history_market_service(series_ids.clone().unwrap(), HistoryInterval::OneHour).await;
    service.sync_instrument().await?;
    let initial_service = service.clone();
    if let Err(e) = initial_service.initial_history_data().await {
        error!("Error initializing polymarket history data: {:?}", e);
    }

    let sync_history_job = service.clone();
    let _ = cron_job!("1 2 * * * *", move |_uuid, _locked| {
        let each_sync = sync_history_job.clone();
        Box::pin(async move {
            //TODO: 正常后，改成debug level
            info!("start fetch last hour history from polymarket server");
            if let Err(e) = each_sync.fetch_latest_history().await {
                error!("Error when async polymarket instruments with server: {}", e);
            }
        })
    });

    let check_history_job = service.clone();

    let _ = cron_job!("0 18 */6 * * *", move |_uuid, _locked| {
        let each_sync = check_history_job.clone();
        Box::pin(async move {
            //TODO: 正常后，改成debug level
            info!("start check polymarket history data");
            if let Err(e) = each_sync.check_history_data().await {
                error!("Error when check polymarket history data: {}", e);
            }
            info!("finish check polymarket history data");
        })
    });

    let mut polymarket_sync_history_service_map = HashMap::new();
    polymarket_sync_history_service_map.insert(InstrumentType::PolymarketToken, PolyMarketSyncHistoryService::new(service.clone()));
    let res = PolyMarketSyncInstrumentService::for_production(series_ids.unwrap(), polymarket_sync_history_service_map);

    let sync_instrument_service = res.clone();
    let _ = cron_job!("0 8 */6 * * *", move |_uuid, _locked| {
        let sync_instrument = sync_instrument_service.clone();
        Box::pin(async move {
            info!("start to sync polymarket instrument");
            if let Err(e) = sync_instrument.sync_instrument().await {
                error!("Error when  sync polymarket instrument: {}", e);
            }
            info!("finish sync polymarket instrument");
        })
    });

    Ok(res)
}

use crate::config::get_config;
use crate::errors::YuError;
use crate::polymarket::service::{SeriesHistoryMarketService, default_series_history_market_service};
use log::error;
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
pub async fn start_polymarket_sync_series_job() -> Result<SeriesHistoryMarketService, YuError> {
    let app_config = get_config();
    let sync_server_config = app_config.sync_server.clone();
    if sync_server_config.is_none() {
        return Err(YuError::ConfigError("sync_server_config".to_string()));
    }
    let series_ids = sync_server_config.unwrap().series_ids;
    if series_ids.is_none() {
        return Err(YuError::ConfigError("series_ids".to_string()));
    }
    let service = default_series_history_market_service(series_ids.unwrap(), HistoryInterval::OneHour).await;
    service.sync_instrument().await?;
    let initial_service = service.clone();
    tokio::spawn(async move {
        if let Err(e) = initial_service.initial_history_data().await {
            error!("Error initializing polymarket history data: {:?}", e);
        }
    });

    Ok(service)
}

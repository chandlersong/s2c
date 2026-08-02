use crate::errors::YuError;
use crate::polymarket::service::{SeriesHistoryMarketService, new_series_history_market_service};
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::default_polymarket_api;

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
    todo!("1. 初始化现有的数据。");
}

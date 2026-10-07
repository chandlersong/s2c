use crate::binance::bn_dashboard::BinanceDashboard;
use crate::config::get_config;
use crate::cron_job;
use crate::errors::YuError;
use log::{error, info};
use std::sync::Arc;
use tokio::sync::watch;

pub async fn start_sync_job() -> Result<Arc<BinanceDashboard>, YuError> {
    let config = get_config();
    let dash_board = Arc::new(BinanceDashboard::new(config.get_data_retention_hours(), None));
    let snapshot = dash_board.execute().await?;
    let (dash_board_watch, _) = watch::channel(snapshot);
    let dash_board_refresh = dash_board.clone();
    let dashboard_watch_sender = dash_board_watch.clone();
    let _ = cron_job!("0 01 * * * *", move |_uuid, _locked| {
        let dash_board_job = dash_board_refresh.clone();
        let dashboard_watch_refresher = dashboard_watch_sender.clone();
        Box::pin(async move {
            info!("start refresh binance exchange info");
            match dash_board_job.clone().execute().await {
                Ok(snapshot) => {
                    if let Err(e) = dashboard_watch_refresher.send(snapshot) {
                        error!("Failed to send updated snapshot to channel: {}", e);
                    } else {
                        info!("BinanceDashboard snapshot updated and sent to channel");
                    }
                }
                Err(_) => {
                    error!("Failed to refresh binance dash_board");
                }
            }
        })
    });
    Ok(dash_board)
}

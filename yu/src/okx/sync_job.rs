use crate::cron_job;
use crate::errors::YuError;
use crate::okx::duckdb_tables::initial_okx_tables;
use crate::okx::service::OptionService;
use log::{error, info};
use std::sync::Arc;

pub async fn start_okx_option_service() -> Result<Arc<OptionService>, YuError> {
    initial_okx_tables(None)?;

    let service = Arc::new(OptionService::default());
    service.start().await?;
    if let Err(e) = service.initial_candle(0, None).await {
        eprintln!("Error initializing okx candle: {:?}", e);
    }
    // service.initial_instruments().await?;
    let check_history_job = service.clone();

    let _ = cron_job!("0 18 */6 * * *", move |_uuid, _locked| {
        let each_sync = check_history_job.clone();
        Box::pin(async move {
            //TODO: 正常后，改成debug level
            info!("start check okx history data");
            if let Err(e) = each_sync.check_history_data().await {
                error!("Error when check okx history data: {}", e);
            };
            info!("finish check okx history data");
        })
    });
    Ok(service)
}

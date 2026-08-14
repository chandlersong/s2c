use crate::errors::YuError;
use crate::okx::duckdb_tables::initial_okx_tables;
use crate::okx::service::OptionService;
use std::sync::Arc;

pub async fn start_okx_option_service() -> Result<Arc<OptionService>, YuError> {
    initial_okx_tables(None)?;

    let service = Arc::new(OptionService::default());
    service.start().await?;
    // service.initial_candle(0).await?;
    // service.initial_instruments().await?;
    Ok(service)
}

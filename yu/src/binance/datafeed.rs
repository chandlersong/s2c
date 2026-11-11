use crate::backtest::core::VectorDataFeedTrait;
use crate::errors::YuError;
use async_trait::async_trait;
use polars::prelude::DataFrame;

pub struct SpotHourlyDataFeed {}

#[async_trait]
impl VectorDataFeedTrait for SpotHourlyDataFeed {
    async fn get_all_data(&self) -> Result<DataFrame, YuError> {
        todo!()
    }
}

use crate::duck_db::DuckDBPO;
use crate::sync::sync_server::grpc_sync::PolyMarketHistory;
use duckdb::appender_params_from_iter;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyMarketHistoryPo {
    pub asset_id: String,
    pub timestamp: u64,
    pub price: f64,
}

impl DuckDBPO for PolyMarketHistoryPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.asset_id as &dyn duckdb::ToSql,
            &self.timestamp as &dyn duckdb::ToSql,
            &self.price as &dyn duckdb::ToSql,
        ])
    }
}

impl From<PolyMarketHistory> for PolyMarketHistoryPo {
    fn from(history: PolyMarketHistory) -> Self {
        let asset_id = history.asset_id.clone();
        Self {
            asset_id,
            timestamp: history.timestamp,
            price: history.price,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyMarketAssetInfoPo {
    pub series_id: String,
    pub series_slug: String,
    pub event_id: String,
    pub event_slug: String,
    pub market_id: String,
    pub market_slug: String,
    pub asset_id: String,
    pub asset_slug: String,
}

impl DuckDBPO for PolyMarketAssetInfoPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.series_id as &dyn duckdb::ToSql,
            &self.series_slug as &dyn duckdb::ToSql,
            &self.event_id as &dyn duckdb::ToSql,
            &self.event_slug as &dyn duckdb::ToSql,
            &self.market_id as &dyn duckdb::ToSql,
            &self.market_slug as &dyn duckdb::ToSql,
            &self.asset_id as &dyn duckdb::ToSql,
            &self.asset_slug as &dyn duckdb::ToSql,
        ])
    }
}

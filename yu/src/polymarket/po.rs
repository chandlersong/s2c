use crate::duck_db::DuckDBPO;
use crate::sync::models::grpc_sync::PolyMarketHistory;
use bon::Builder;
use duckdb::appender_params_from_iter;
use li::tools::time::unix_2_readable;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Builder)]
pub struct PolyMarketHistoryPo {
    pub instrument_id: u64,
    pub timestamp: u64,
    pub price: f64,
}

impl DuckDBPO for PolyMarketHistoryPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.instrument_id as &dyn duckdb::ToSql,
            &self.timestamp as &dyn duckdb::ToSql,
            &self.price as &dyn duckdb::ToSql,
        ])
    }
}

impl PolyMarketHistoryPo {
    pub fn from_vo(instrument_id: u64, history: PolyMarketHistory) -> Self {
        Self {
            instrument_id,
            timestamp: history.timestamp,
            price: history.price,
        }
    }
}

// Provide a Display implementation so the PO can be printed with `{}` and `.to_string()`
impl std::fmt::Display for PolyMarketHistoryPo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PolyMarketHistoryPo {{ instrument_id: {}, timestamp: {}, price: {} }}",
            self.instrument_id,
            unix_2_readable(&(self.timestamp * 1000)),
            self.price
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyMarketInstrumentPo {
    pub id: u64,
    pub series_id: String,
    pub series_slug: String,
    pub event_id: String,
    pub event_slug: String,
    pub market_id: String,
    pub market_slug: String,
    pub asset_id: String,
    pub asset_slug: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl DuckDBPO for PolyMarketInstrumentPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.series_id as &dyn duckdb::ToSql,
            &self.series_slug as &dyn duckdb::ToSql,
            &self.event_id as &dyn duckdb::ToSql,
            &self.event_slug as &dyn duckdb::ToSql,
            &self.market_id as &dyn duckdb::ToSql,
            &self.market_slug as &dyn duckdb::ToSql,
            &self.asset_id as &dyn duckdb::ToSql,
            &self.asset_slug as &dyn duckdb::ToSql,
            &self.start_ms as &dyn duckdb::ToSql,
            &self.end_ms as &dyn duckdb::ToSql,
        ])
    }
}

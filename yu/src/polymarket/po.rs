use crate::duck_db::DuckDBPO;
use crate::sync::sync_server::grpc_sync::PolyMarketHistory;
use duckdb::appender_params_from_iter;
use prost::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyMarketHistoryPo {
    pub assert_id: String,
    pub timestamp: u64,
    pub payload: Vec<u8>,
}

impl DuckDBPO for PolyMarketHistoryPo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.assert_id as &dyn duckdb::ToSql,
            &self.timestamp as &dyn duckdb::ToSql,
            &self.payload as &dyn duckdb::ToSql,
        ])
    }
}

impl From<PolyMarketHistory> for PolyMarketHistoryPo {
    fn from(history: PolyMarketHistory) -> Self {
        let assert_id = history.asset_id.clone();
        Self {
            assert_id,
            timestamp: history.timestamp,
            payload: history.encode_to_vec(),
        }
    }
}

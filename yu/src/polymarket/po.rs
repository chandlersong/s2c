use crate::duck_db::DuckDBPO;
use duckdb::appender_params_from_iter;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyMarketHistoryPo {
    pub assert_id: String,
    pub timestamp: i64,
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

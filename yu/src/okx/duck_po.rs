use crate::duck_db::DuckDBPO;
use duckdb::appender_params_from_iter;
use serde::{Deserialize, Serialize};
use yue::okx::models::common::CandleResponse;
use yue::tools::get_snow_flake_id_u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OkxKlinePo {
    pub id: u64,
    pub inst_id: String,
    pub ts: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub vol: f64,
    pub vol_ccy: f64,
    pub vol_ccy_quote: f64,
    pub confirm: u8,
}

impl OkxKlinePo {
    pub fn from_kline_response(inst_id: &str, response: CandleResponse) -> Vec<Self> {
        let mut res = Vec::<Self>::new();
        for candle in response.data {
            res.push(Self {
                id: get_snow_flake_id_u64(),
                inst_id: inst_id.to_string(),
                ts: candle[0].parse().unwrap(),
                open: candle[1].parse().unwrap(),
                high: candle[2].parse().unwrap(),
                low: candle[3].parse().unwrap(),
                close: candle[4].parse().unwrap(),
                vol: candle[5].parse().unwrap(),
                vol_ccy: candle[6].parse().unwrap(),
                vol_ccy_quote: candle[7].parse().unwrap(),
                confirm: candle[8].parse().unwrap(),
            });
        }
        res
    }
}

impl DuckDBPO for OkxKlinePo {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.inst_id as &dyn duckdb::ToSql,
            &self.ts as &dyn duckdb::ToSql,
            &self.open as &dyn duckdb::ToSql,
            &self.high as &dyn duckdb::ToSql,
            &self.low as &dyn duckdb::ToSql,
            &self.close as &dyn duckdb::ToSql,
            &self.vol as &dyn duckdb::ToSql,
            &self.vol_ccy as &dyn duckdb::ToSql,
            &self.vol_ccy_quote as &dyn duckdb::ToSql,
            &self.confirm as &dyn duckdb::ToSql,
        ])
    }
}

use crate::postgresql_db::CopyInsertable;
use sqlx::Row;
use yue::tools::get_snow_flake_id_u64;

#[derive(Debug, Clone)]
pub struct LocalOkxInstrumentPo {
    pub id: u64,
    pub server_id: u64,
    pub inst_identify: String,
    pub inst_type: String,
    pub inst_family: Option<String>,
    pub base_ccy: String,
    pub quote_ccy: Option<String>,
    pub settle_ccy: Option<String>,
    pub list_time: Option<u64>,
    pub exp_time: Option<u64>,
    pub tick_sz: Option<f64>,
    pub lot_sz: Option<f64>,
    pub min_sz: Option<f64>,
    pub alias: Option<String>,
    pub state: Option<String>,
    pub inst_id_code: Option<String>,
    pub inst_category: Option<String>,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalOkxInstrumentPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        let server_id_i64: i64 = row.try_get("server_id")?;
        let inst_identify: String = row.try_get("inst_identify")?;
        let inst_type: String = row.try_get("inst_type")?;
        let inst_family: Option<String> = row.try_get("inst_family")?;
        let base_ccy: String = row.try_get("base_ccy")?;
        let quote_ccy: Option<String> = row.try_get("quote_ccy")?;
        let settle_ccy: Option<String> = row.try_get("settle_ccy")?;
        let list_time_i64: Option<i64> = row.try_get("list_time")?;
        let exp_time_i64: Option<i64> = row.try_get("exp_time")?;
        let tick_sz: Option<f64> = row.try_get("tick_sz")?;
        let lot_sz: Option<f64> = row.try_get("lot_sz")?;
        let min_sz: Option<f64> = row.try_get("min_sz")?;
        let alias: Option<String> = row.try_get("alias")?;
        let state: Option<String> = row.try_get("state")?;
        let inst_id_code: Option<String> = row.try_get("inst_id_code")?;
        let inst_category: Option<String> = row.try_get("inst_category")?;

        Ok(Self {
            id: id_i64 as u64,
            server_id: server_id_i64 as u64,
            inst_identify,
            inst_type,
            inst_family,
            base_ccy,
            quote_ccy,
            settle_ccy,
            list_time: list_time_i64.map(|v| v as u64),
            exp_time: exp_time_i64.map(|v| v as u64),
            tick_sz,
            lot_sz,
            min_sz,
            alias,
            state,
            inst_id_code,
            inst_category,
        })
    }
}

#[derive(Clone, Debug)]
pub struct LocalOkxKlinePo {
    pub id: u64,
    pub instrument_id: u64, // local instrument id
    pub timestamp: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub vol: f64,
    pub vol_ccy: f64,
    pub vol_ccy_quote: f64,
    pub confirm: u32,
    pub batch_timestamp: u64,
}

impl LocalOkxKlinePo {
    pub fn from_okx_kline(k: crate::sync::models::grpc_sync::OkxKline, local_inst_id: u64, batch_timestamp: u64) -> Self {
        LocalOkxKlinePo {
            id: get_snow_flake_id_u64(),
            instrument_id: local_inst_id,
            timestamp: k.ts,
            open: k.open,
            high: k.high,
            low: k.low,
            close: k.close,
            vol: k.vol,
            vol_ccy: k.vol_ccy,
            vol_ccy_quote: k.vol_ccy_quote,
            confirm: k.confirm,
            batch_timestamp,
        }
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalOkxKlinePo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        let instrument_id_i64: i64 = row.try_get("instrument_id")?;

        let timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("timestamp")?;
        let timestamp = timestamp_dt.timestamp() as u64;

        let open: f64 = row.try_get("open")?;
        let high: f64 = row.try_get("high")?;
        let low: f64 = row.try_get("low")?;
        let close: f64 = row.try_get("close")?;
        let vol: f64 = row.try_get("vol")?;
        let vol_ccy: f64 = row.try_get("vol_ccy")?;
        let vol_ccy_quote: f64 = row.try_get("vol_ccy_quote")?;
        let confirm_i32: i32 = row.try_get("confirm")?;

        let batch_timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("batch_timestamp")?;
        let batch_timestamp = batch_timestamp_dt.timestamp() as u64;

        Ok(Self {
            id: id_i64 as u64,
            instrument_id: instrument_id_i64 as u64,
            timestamp,
            open,
            high,
            low,
            close,
            vol,
            vol_ccy,
            vol_ccy_quote,
            confirm: confirm_i32 as u32,
            batch_timestamp,
        })
    }
}

impl CopyInsertable for LocalOkxKlinePo {
    fn columns() -> &'static str {
        "id,instrument_id,timestamp,open,high,low,close,vol,vol_ccy,vol_ccy_quote,confirm,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        use chrono::{LocalResult, TimeZone};
        let ts_secs = (self.timestamp / 1000) as i64;
        let ts_nanos = ((self.timestamp % 1000) * 1_000_000) as u32;
        let ts_dt = match chrono::Utc.timestamp_opt(ts_secs, ts_nanos) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let ts = ts_dt.to_rfc3339();

        let bts_secs = (self.batch_timestamp / 1000) as i64;
        let bts_nanos = ((self.batch_timestamp % 1000) * 1_000_000) as u32;
        let bts_dt = match chrono::Utc.timestamp_opt(bts_secs, bts_nanos) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let bts = bts_dt.to_rfc3339();

        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            self.id,
            self.instrument_id,
            ts,
            self.open,
            self.high,
            self.low,
            self.close,
            self.vol,
            self.vol_ccy,
            self.vol_ccy_quote,
            self.confirm,
            bts
        )
    }
}

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
    pub candle_begin_time: u64,
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
            candle_begin_time: k.ts,
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

        let timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("candle_begin_time")?;
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
            candle_begin_time: timestamp,
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
        "id,instrument_id,candle_begin_time,open,high,low,close,vol,vol_ccy,vol_ccy_quote,confirm,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        use chrono::{LocalResult, TimeZone};
        let ts_secs = (self.candle_begin_time / 1000) as i64;
        let ts_nanos = ((self.candle_begin_time % 1000) * 1_000_000) as u32;
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

#[derive(Clone, Debug)]
pub struct LocalOkxOptionSummaryPo {
    pub id: u64,
    pub instrument_id: u64,
    pub inst_identify: String,
    pub inst_type: String,
    pub uly: Option<String>,
    pub acquire_ts: u64,
    pub server_ts: u64,
    pub ask_vol: Option<f64>,
    pub bid_vol: Option<f64>,
    pub delta: Option<f64>,
    pub delta_bs: Option<f64>,
    pub fwd_px: Option<f64>,
    pub gamma: Option<f64>,
    pub gamma_bs: Option<f64>,
    pub lever: Option<f64>,
    pub mark_vol: Option<f64>,
    pub real_vol: Option<f64>,
    pub vol_lv: Option<f64>,
    pub theta: Option<f64>,
    pub theta_bs: Option<f64>,
    pub vega: Option<f64>,
    pub vega_bs: Option<f64>,
    pub batch_timestamp: u64,
}

impl LocalOkxOptionSummaryPo {
    pub fn from_okx_summary(s: crate::sync::models::grpc_sync::OptionSummary, local_inst_id: u64, batch_timestamp: u64) -> Self {
        LocalOkxOptionSummaryPo {
            id: get_snow_flake_id_u64(),
            instrument_id: local_inst_id,
            inst_identify: s.inst_identify,
            inst_type: s.inst_type,
            uly: if s.uly.is_empty() { None } else { Some(s.uly) },
            acquire_ts: s.acquire_ts,
            server_ts: s.server_ts,
            ask_vol: s.ask_vol,
            bid_vol: s.bid_vol,
            delta: s.delta,
            delta_bs: s.delta_bs,
            fwd_px: s.fwd_px,
            gamma: s.gamma,
            gamma_bs: s.gamma_bs,
            lever: s.lever,
            mark_vol: s.mark_vol,
            real_vol: s.real_vol,
            vol_lv: s.vol_lv,
            theta: s.theta,
            theta_bs: s.theta_bs,
            vega: s.vega,
            vega_bs: s.vega_bs,
            batch_timestamp,
        }
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalOkxOptionSummaryPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        let instrument_id_i64: i64 = row.try_get("instrument_id")?;
        let inst_identify: String = row.try_get("inst_identify")?;
        let inst_type: String = row.try_get("inst_type")?;
        let uly: Option<String> = row.try_get("uly")?;

        let acquire_ts_dt: chrono::DateTime<chrono::Utc> = row.try_get("acquire_ts")?;
        let acquire_ts = acquire_ts_dt.timestamp() as u64;
        let server_ts_dt: chrono::DateTime<chrono::Utc> = row.try_get("server_ts")?;
        let server_ts = server_ts_dt.timestamp() as u64;

        let ask_vol: Option<f64> = row.try_get("ask_vol")?;
        let bid_vol: Option<f64> = row.try_get("bid_vol")?;
        let delta: Option<f64> = row.try_get("delta")?;
        let delta_bs: Option<f64> = row.try_get("delta_bs")?;
        let fwd_px: Option<f64> = row.try_get("fwd_px")?;
        let gamma: Option<f64> = row.try_get("gamma")?;
        let gamma_bs: Option<f64> = row.try_get("gamma_bs")?;
        let lever: Option<f64> = row.try_get("lever")?;
        let mark_vol: Option<f64> = row.try_get("mark_vol")?;
        let real_vol: Option<f64> = row.try_get("real_vol")?;
        let vol_lv: Option<f64> = row.try_get("vol_lv")?;
        let theta: Option<f64> = row.try_get("theta")?;
        let theta_bs: Option<f64> = row.try_get("theta_bs")?;
        let vega: Option<f64> = row.try_get("vega")?;
        let vega_bs: Option<f64> = row.try_get("vega_bs")?;

        let batch_timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("batch_timestamp")?;
        let batch_timestamp = batch_timestamp_dt.timestamp() as u64;

        Ok(Self {
            id: id_i64 as u64,
            instrument_id: instrument_id_i64 as u64,
            inst_identify,
            inst_type,
            uly,
            acquire_ts,
            server_ts,
            ask_vol,
            bid_vol,
            delta,
            delta_bs,
            fwd_px,
            gamma,
            gamma_bs,
            lever,
            mark_vol,
            real_vol,
            vol_lv,
            theta,
            theta_bs,
            vega,
            vega_bs,
            batch_timestamp,
        })
    }
}

impl CopyInsertable for LocalOkxOptionSummaryPo {
    fn columns() -> &'static str {
        "id,instrument_id,inst_identify,inst_type,uly,acquire_ts,server_ts,ask_vol,bid_vol,delta,delta_bs,fwd_px,gamma,gamma_bs,lever,mark_vol,real_vol,vol_lv,theta,theta_bs,vega,vega_bs,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        use chrono::{LocalResult, TimeZone};
        let acquire_secs = (self.acquire_ts / 1000) as i64;
        let acquire_nanos = ((self.acquire_ts % 1000) * 1_000_000) as u32;
        let acquire_dt = match chrono::Utc.timestamp_opt(acquire_secs, acquire_nanos) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let acquire = acquire_dt.to_rfc3339();

        let server_secs = (self.server_ts / 1000) as i64;
        let server_nanos = ((self.server_ts % 1000) * 1_000_000) as u32;
        let server_dt = match chrono::Utc.timestamp_opt(server_secs, server_nanos) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let server = server_dt.to_rfc3339();

        let batch_secs = (self.batch_timestamp / 1000) as i64;
        let batch_nanos = ((self.batch_timestamp % 1000) * 1_000_000) as u32;
        let batch_dt = match chrono::Utc.timestamp_opt(batch_secs, batch_nanos) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let bts = batch_dt.to_rfc3339();

        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.id,
            self.instrument_id,
            self.inst_identify,
            self.inst_type,
            self.uly.clone().unwrap_or_default(),
            acquire,
            server,
            self.ask_vol.map_or(String::from(""), |v| v.to_string()),
            self.bid_vol.map_or(String::from(""), |v| v.to_string()),
            self.delta.map_or(String::from(""), |v| v.to_string()),
            self.delta_bs.map_or(String::from(""), |v| v.to_string()),
            self.fwd_px.map_or(String::from(""), |v| v.to_string()),
            self.gamma.map_or(String::from(""), |v| v.to_string()),
            self.gamma_bs.map_or(String::from(""), |v| v.to_string()),
            self.lever.map_or(String::from(""), |v| v.to_string()),
            self.mark_vol.map_or(String::from(""), |v| v.to_string()),
            self.real_vol.map_or(String::from(""), |v| v.to_string()),
            self.vol_lv.map_or(String::from(""), |v| v.to_string()),
            self.theta.map_or(String::from(""), |v| v.to_string()),
            self.theta_bs.map_or(String::from(""), |v| v.to_string()),
            self.vega.map_or(String::from(""), |v| v.to_string()),
            self.vega_bs.map_or(String::from(""), |v| v.to_string()),
            bts
        )
    }
}

use crate::postgresql_db::CopyInsertable;
use crate::sync::models::grpc_sync::PolyMarketHistory;
use sqlx::Row;
use yue::tools::get_snow_flake_id_u64;

#[derive(Debug)]
pub struct LocalPolyMarketInstrumentPo {
    pub id: u64,
    pub server_id: u64,
    pub series_id: String,
    pub series_slug: String,
    pub event_id: String,
    pub event_slug: String,
    pub market_id: String,
    pub market_slug: String,
    pub assert_id: String,
    pub assert_slug: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalPolyMarketInstrumentPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        let server_id_i64: i64 = row.try_get("server_id")?;
        let series_id: String = row.try_get("series_id")?;
        let series_slug: String = row.try_get("series_slug")?;
        let event_id: String = row.try_get("event_id")?;
        let event_slug: String = row.try_get("event_slug")?;
        let market_id: String = row.try_get("market_id")?;
        let market_slug: String = row.try_get("market_slug")?;
        let assert_id: String = row.try_get("assert_id")?;
        let assert_slug: String = row.try_get("assert_slug")?;
        let start_ms_i64: i64 = row.try_get("start_ms")?;
        let end_ms_i64: i64 = row.try_get("end_ms")?;

        Ok(Self {
            id: id_i64 as u64,
            server_id: server_id_i64 as u64,
            series_id,
            series_slug,
            event_id,
            event_slug,
            market_id,
            market_slug,
            assert_id,
            assert_slug,
            start_ms: start_ms_i64 as u64,
            end_ms: end_ms_i64 as u64,
        })
    }
}

///
/// 这里的时间戳，都是seconds
///
#[derive(Clone, Debug)]
pub struct LocalPolyMarketHistoryPo {
    pub id: u64,
    pub inst_id: u64,
    pub timestamp: u64,
    pub price: f64,
    pub batch_timestamp: u64,
}

impl LocalPolyMarketHistoryPo {
    pub fn from_polymarket_history(history: PolyMarketHistory, batch_timestamp: u64) -> LocalPolyMarketHistoryPo {
        LocalPolyMarketHistoryPo {
            id: get_snow_flake_id_u64(),
            inst_id: history.inst_id,
            timestamp: history.timestamp,
            price: history.price,
            batch_timestamp,
        }
    }
}

// 手动实现 FromRow，支持从 timestamptz/BigInt 等类型读取并转换为 u64
impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalPolyMarketHistoryPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        // 注意列名是 asset_id
        let asset_id: i64 = row.try_get("asset_id")?;

        // timestamp 在数据库中为 timestamptz 时，使用 chrono::DateTime<Utc> 读取并转换为秒
        let timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("timestamp")?;
        let timestamp = timestamp_dt.timestamp() as u64;

        let price: f64 = row.try_get("price")?;

        let batch_timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("batch_timestamp")?;
        let batch_timestamp = batch_timestamp_dt.timestamp() as u64;

        Ok(Self {
            id: id_i64 as u64,
            inst_id: asset_id as u64,
            timestamp,
            price,
            batch_timestamp,
        })
    }
}

impl CopyInsertable for LocalPolyMarketHistoryPo {
    fn columns() -> &'static str {
        "id,asset_id,timestamp,price,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        // 将 epoch 秒转换为 Postgres 可识别的 timestamptz 文本（ISO 8601）
        use chrono::{LocalResult, TimeZone};
        let ts_dt = match chrono::Utc.timestamp_opt(self.timestamp as i64, 0) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let ts = ts_dt.to_rfc3339();
        let bts_dt = match chrono::Utc.timestamp_opt(self.batch_timestamp as i64, 0) {
            LocalResult::Single(dt) => dt,
            _ => chrono::Utc.timestamp_opt(0, 0).single().unwrap(),
        };
        let bts = bts_dt.to_rfc3339();
        format!("{},{},{},{},{}", self.id, self.inst_id, ts, self.price, bts)
    }
}

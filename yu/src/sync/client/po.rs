use crate::postgresql_db::CopyInsertable;
use crate::sync::models::grpc_sync::PolyMarketHistory;
use sqlx::{FromRow, Row};
use yue::tools::get_snow_flake_id_u64;

#[derive(Debug, FromRow)]
pub struct LocalPolyMarketAssetInfoPo {
    pub series_id: String,
    pub series_slug: String,
    pub event_id: String,
    pub event_slug: String,
    pub market_id: String,
    pub market_slug: String,
    pub assert_id: String,
    pub assert_slug: String,
}

///
/// 这里的时间戳，都是seconds
///
#[derive(Clone, Debug)]
pub struct LocalPolyMarketHistoryPo {
    pub id: u64,
    pub asset_id: String,
    pub timestamp: u64,
    pub price: f64,
    pub batch_timestamp: u64,
}

impl LocalPolyMarketHistoryPo {
    pub fn from_polymarket_history(history: PolyMarketHistory, batch_timestamp: u64) -> LocalPolyMarketHistoryPo {
        LocalPolyMarketHistoryPo {
            id: get_snow_flake_id_u64(),
            asset_id: history.asset_id,
            timestamp: history.timestamp,
            price: history.price,
            batch_timestamp,
        }
    }
}

// 手动实现 FromRow，支持从 timestamptz/BigInt 等类型读取并转换为 u64
impl<'r> FromRow<'r, sqlx::postgres::PgRow> for LocalPolyMarketHistoryPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        let id_i64: i64 = row.try_get("id")?;
        // 注意列名是 asset_id
        let asset_id: String = row.try_get("asset_id")?;

        // timestamp 在数据库中为 timestamptz 时，使用 chrono::DateTime<Utc> 读取并转换为秒
        let timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("timestamp")?;
        let timestamp = timestamp_dt.timestamp() as u64;

        let price: f64 = row.try_get("price")?;

        let batch_timestamp_dt: chrono::DateTime<chrono::Utc> = row.try_get("batch_timestamp")?;
        let batch_timestamp = batch_timestamp_dt.timestamp() as u64;

        Ok(Self {
            id: id_i64 as u64,
            asset_id,
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
        format!("{},{},{},{},{}", self.id, self.asset_id, ts, self.price, bts)
    }
}

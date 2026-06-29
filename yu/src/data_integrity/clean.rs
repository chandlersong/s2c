use crate::binance::db_constants::BinanceTables;
use crate::duck_db::DBProvider;
use async_trait::async_trait;
use duckdb::{params, DuckdbConnectionManager};
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info};
use r2d2::PooledConnection;
use yue::models::HistoryInterval;

#[derive(Clone)]
struct CleanInfo {
    pub table_name: String,
    pub time_col_name: String,
}

#[derive(Clone)]
pub struct TableCleaner {
    info: Vec<CleanInfo>,
    retain_ms: u64,
    db_provider: DBProvider,
}

impl TableCleaner {
    ///
    /// 代码即配置。因为现阶段没必要做成可配置。就这样来弄了。
    ///
    pub fn new(retain_ms: u64) -> Self {
        let info = vec![
            CleanInfo {
                table_name: BinanceTables::SpotKline.table_name(),
                time_col_name: "candle_begin_time".to_string(),
            },
            CleanInfo {
                table_name: BinanceTables::SwapKline.table_name(),
                time_col_name: "candle_begin_time".to_string(),
            },
            CleanInfo {
                table_name: BinanceTables::SwapFundingRate.table_name(),
                time_col_name: "funding_time".to_string(),
            },
            CleanInfo {
                table_name: BinanceTables::SpotOrderEvents.table_name(),
                time_col_name: "event_time".to_string(),
            },
            CleanInfo {
                table_name: BinanceTables::SpotTrade.table_name(),
                time_col_name: "event_time".to_string(),
            },
        ];
        Self {
            info,
            retain_ms,
            db_provider: DBProvider::default(),
        }
    }

    #[cfg(test)]
    pub fn new_with_db(retain_ms: u64, db_provider: DBProvider) -> Self {
        let info = vec![CleanInfo {
            table_name: BinanceTables::SpotKline.table_name(),
            time_col_name: "candle_begin_time".to_string(),
        }];
        Self {
            info,
            retain_ms,
            db_provider,
        }
    }

    pub fn batch_delete(conn: &PooledConnection<DuckdbConnectionManager>, table_name: &str, column_name: &str, earliest: u64) -> Result<(), LiError> {
        let sql = format!("DELETE FROM {} WHERE {} < ? ", table_name, column_name);
        if let Err(e) = conn.execute(&sql, params![earliest]) {
            error!("TableCleaner delete failed: table={}, error={}", table_name, e);
            return Err(LiError::CustomError(format!("Failed to execute clean sql: {}", e)));
        };
        Ok(())
    }
}

#[async_trait]
impl AsyncRepeatTask for TableCleaner {
    async fn initial_data(&self) -> Result<(), LiError> {
        self.execute().await
    }

    ///
    /// # 确定earliest时间戳
    /// 1. 现在utc事件-retain_ms。然后通过HistoryInterval::HOUR拉截取事件
    /// 2. 然后loop本地的info。对于每个表，执行delete from table where time_col_name < earliest
    ///
    async fn execute(&self) -> Result<(), LiError> {
        // info!("TableCleaner executing");
        let now_ms = unix_time_now_u64_utc();
        let earliest_raw = now_ms.saturating_sub(self.retain_ms);
        let earliest = HistoryInterval::OneHour.get_close_unix_ms(earliest_raw);
        let conn = self
            .db_provider
            .acquire()
            .map_err(|e| LiError::CustomError(format!("Failed to acquire db connection: {}", e)))?;
        if let Err(e) = conn.execute_batch("SET preserve_insertion_order=false") {
            error!("TableCleaner failed to set preserve_insertion_order: error={}", e);
        }
        for item in &self.info {
            if let Err(e) = Self::batch_delete(&conn, &item.table_name, &item.time_col_name, earliest) {
                error!("TableCleaner delete failed: table={}, error={}", item.table_name, e);
                return Err(LiError::CustomError(format!("Failed to execute clean sql: {}", e)));
            }
        }
        if let Err(e) = conn.execute_batch("SET preserve_insertion_order=true") {
            error!("TableCleaner failed to set preserve_insertion_order: error={}", e);
        }
        info!("TableCleaner finished");
        Ok(())
    }

    fn task_name(&self) -> &str {
        "TableCleaner"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_memory_db_provider;

    #[tokio::test]
    async fn test_execute_deletes_old_rows() {
        let db_provider = create_memory_db_provider();
        let cleaner = TableCleaner::new_with_db(0, db_provider.clone());
        let conn = db_provider.acquire().unwrap();
        let table = BinanceTables::SpotKline.table_name();
        conn.execute(&format!("CREATE TABLE {} (candle_begin_time BIGINT)", table), []).unwrap();
        let now = unix_time_now_u64_utc();
        let old_ts = now.saturating_sub(10 * 60 * 60 * 1000);
        let future_ts = now.saturating_add(10 * 60 * 60 * 1000);
        conn.execute(&format!("INSERT INTO {} (candle_begin_time) VALUES (?)", table), params![old_ts])
            .unwrap();
        conn.execute(&format!("INSERT INTO {} (candle_begin_time) VALUES (?)", table), params![future_ts])
            .unwrap();

        cleaner.execute().await.unwrap();

        let count: i64 = conn
            .prepare(&format!("SELECT COUNT(*) FROM {}", table))
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_execute_keeps_recent_rows() {
        let db_provider = create_memory_db_provider();
        let cleaner = TableCleaner::new_with_db(24 * 60 * 60 * 1000, db_provider.clone());
        let conn = db_provider.acquire().unwrap();
        let table = BinanceTables::SpotKline.table_name();
        conn.execute(&format!("CREATE TABLE {} (candle_begin_time BIGINT)", table), []).unwrap();
        let now = unix_time_now_u64_utc();
        let recent_ts = now.saturating_sub(60 * 60 * 1000);
        conn.execute(&format!("INSERT INTO {} (candle_begin_time) VALUES (?)", table), params![recent_ts])
            .unwrap();

        cleaner.execute().await.unwrap();

        let count: i64 = conn
            .prepare(&format!("SELECT COUNT(*) FROM {}", table))
            .unwrap()
            .query_row([], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}

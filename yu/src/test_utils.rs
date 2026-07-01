use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use duckdb::{Connection, DuckdbConnectionManager, Result};
use r2d2::Pool;
use rust_decimal::dec;
use rust_decimal::prelude::FromPrimitive;
use std::path::Path;
use std::vec::Vec;
use yue::binance::bn_models::spot_restful::BinanceKline;

/// 用于测试的起始时间戳（2021-01-01 00:00:00 UTC）
pub const TEST_BEGIN_TIMESTAMP: u64 = 1609459200000;

/// 从本地 CSV 路径导入指定表并断言行数等于 expected_count
/// - csv_path 可以是相对或绝对路径，函数会 canonicalize 转为绝对路径
/// - 如果导入或断言失败，会 panic（适合测试场景）
pub fn import_local_csv_and_assert(conn: &Connection, table: &str, csv_path: &Path, expected_count: i64) -> Result<(), YuError> {
    let abs = match std::fs::canonicalize(csv_path) {
        Ok(v) => v,
        Err(_) => {
            return Err(YuError::new(format!("CSV file not found: {}", csv_path.display()).as_str()));
        }
    };
    let copy_sql = format!("COPY {} FROM '{}' (FORMAT CSV, HEADER);", table, abs.display());
    conn.execute_batch(&copy_sql)?;

    let query = format!("SELECT count(*) FROM {}", table);
    let count: i64 = conn.prepare(&query)?.query_row([], |row| row.get(0))?;
    assert_eq!(count, expected_count, "imported row count mismatch");
    Ok(())
}

/// 生成用于测试的 Vec<Kline>，可指定起始时间、间隔和数量
/// 可以通过close来控制一些判断
pub fn generate_test_kline_vec(start_time: u64, interval_ms: u64, close: f64, count: usize) -> Vec<BinanceKline> {
    (0..count)
        .map(|i| {
            let open_time = start_time + i as u64 * interval_ms;
            let close_time = open_time + interval_ms - 1;
            BinanceKline {
                open_time,
                symbol: None,
                open: dec!(10000.0),
                high: dec!(10100.0),
                low: dec!(9900.0),
                close: rust_decimal::Decimal::from_f64(close).unwrap(),
                volume: dec!(10.0),
                close_time,
                quote_asset_volume: dec!(100500.0),
                number_of_trades: 100,
                taker_buy_base_asset_volume: dec!(5.0),
                taker_buy_quote_asset_volume: dec!(50000.0),
                ignore: "0".to_string(),
            }
        })
        .collect()
}

pub fn create_memory_db_provider() -> DuckDBDSProvider {
    let manager = DuckdbConnectionManager::memory().unwrap();
    let pool = Pool::builder().max_size(4).build(manager).unwrap();
    DuckDBDSProvider::new(pool)
}

pub fn initial_memory_db() -> Pool<DuckdbConnectionManager> {
    let builder = Pool::builder()
        .max_size(2) // 最大连接数
        .min_idle(Some(1)) // 最小空闲连接数
        .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

    builder.build(DuckdbConnectionManager::memory().unwrap()).unwrap()
}

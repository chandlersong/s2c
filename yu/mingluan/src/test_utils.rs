use crate::errors::MingLuanError;
use duckdb::{Connection, Result};
use std::path::Path;
use std::vec::Vec;
use yue::binance::bn_models::BinanceKline;

/// 用于测试的起始时间戳（2021-01-01 00:00:00 UTC）
pub const TEST_BEGIN_TIMESTAMP: u64 = 1609459200000;

/// 从本地 CSV 路径导入指定表并断言行数等于 expected_count
/// - csv_path 可以是相对或绝对路径，函数会 canonicalize 转为绝对路径
/// - 如果导入或断言失败，会 panic（适合测试场景）
pub fn import_local_csv_and_assert(
    conn: &Connection,
    table: &str,
    csv_path: &Path,
    expected_count: i64,
) -> Result<(), MingLuanError> {
    let abs = match std::fs::canonicalize(csv_path) {
        Ok(v) => v,
        Err(_) => {
            return Err(MingLuanError::new(
                format!("CSV file not found: {}", csv_path.display()).as_str(),
            ));
        }
    };
    let copy_sql = format!(
        "COPY {} FROM '{}' (FORMAT CSV, HEADER);",
        table,
        abs.display()
    );
    conn.execute_batch(&copy_sql)?;

    let query = format!("SELECT count(*) FROM {}", table);
    let count: i64 = conn.prepare(&query)?.query_row([], |row| row.get(0))?;
    assert_eq!(count, expected_count, "imported row count mismatch");
    Ok(())
}

/// 生成用于测试的 Vec<Kline>，可指定起始时间、间隔和数量
/// 可以通过close来控制一些判断
pub fn generate_test_kline_vec(
    start_time: u64,
    interval_ms: u64,
    close: f64,
    count: usize,
) -> Vec<BinanceKline> {
    (0..count)
        .map(|i| {
            let open_time = start_time + i as u64 * interval_ms;
            let close_time = open_time + interval_ms - 1;
            BinanceKline {
                open_time,
                open: 10000.0,
                high: 10100.0,
                low: 9900.0,
                close,
                volume: 10.0,
                close_time,
                quote_asset_volume: 100500.0,
                number_of_trades: 100,
                taker_buy_base_asset_volume: 5.0,
                taker_buy_quote_asset_volume: 50000.0,
                ignore: "0".to_string(),
            }
        })
        .collect()
}

use duckdb::{Connection, Result};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 从本地 CSV 路径导入指定表并断言行数等于 expected_count
/// - csv_path 可以是相对或绝对路径，函数会 canonicalize 转为绝对路径
/// - 如果导入或断言失败，会 panic（适合测试场景）
pub fn import_local_csv_and_assert(
    conn: &Connection,
    table: &str,
    csv_path: &Path,
    expected_count: i64,
) -> Result<()> {
    let abs = std::fs::canonicalize(csv_path).expect("canonicalize csv path");
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

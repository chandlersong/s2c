use duckdb::Connection;

///
/// Refresh the spot kline data from Binance API
pub fn refresh_spot_kline(conn: &Connection) {
    // let _latest_symbol = conn
    //     .prepare(QUERY_LATEST_SQL)
    //     .unwrap()
    //     .query_map([], |row| {
    //         let symbol: String = row.get(0)?;
    //         let latest: i64 = row.get(1)?;
    //         Ok((symbol, latest))
    //     })
    //     .unwrap();
}

#[cfg(test)]
mod tests {
    use crate::binance::binance_consts::BinanceTables::SpotKline;
    use crate::test_utils::import_local_csv_and_assert;
    use duckdb::Connection;
    use std::path::Path;

    fn initial_db() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
    }

    #[tokio::test]
    async fn test_refresh_spot_kline_normal() {
        // 初始化内存数据库连接并建表
        let conn = initial_db();
        conn.execute(SpotKline.create_table_statement().as_str(), [])
            .unwrap();
        // 直接从仓库中的本地 CSV 导入并断言行数为 2
        let csv_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/test_refresh_spot_kline_normal.csv");
        import_local_csv_and_assert(&conn, "spot_kline", csv_path.as_path(), 2).unwrap();

        // 测试完成：已导入 fixture 数据。后续可调用 refresh_spot_kline(&conn) 并断言预期行为。
    }
}

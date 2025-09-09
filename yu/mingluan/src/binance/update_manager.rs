use crate::binance::binance_consts::QUERY_LATEST_SQL;
use crate::exchange::ExchangeUpdateManager;
use duckdb::DuckdbConnectionManager;
use r2d2::PooledConnection;
use yue::binance::spots::{KlineInterval, get_all_kline_data};

pub struct BinanceUpdateManager {
    connection: PooledConnection<DuckdbConnectionManager>,
}

impl BinanceUpdateManager {
    pub fn new(connection: PooledConnection<DuckdbConnectionManager>) -> Self {
        BinanceUpdateManager { connection }
    }
}

impl ExchangeUpdateManager for BinanceUpdateManager {
    async fn refresh_spot_kline(&self) -> String {
        let mut stmt = self.connection.prepare(QUERY_LATEST_SQL).unwrap();
        let latest_symbol = stmt
            .query_map([], |row| {
                let symbol: String = row.get(0)?;
                let latest: i64 = row.get(1)?;
                Ok((symbol, latest))
            })
            .unwrap();

        // 打印查询到的每个 symbol 和对应的 latest 时间戳，便于调试
        for entry in latest_symbol {
            match entry {
                Ok((symbol, latest)) => {
                    println!("latest_symbol -> symbol: {}, latest: {}", symbol, latest);
                    // get_all_kline_data(&symbol, KlineInterval::OneHour, None).await;
                }
                Err(e) => eprintln!("error reading latest_symbol row: {}", e),
            }
        }
        String::from("OK")
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::binance_consts::BinanceTables::SpotKline;
    use crate::binance::update_manager::BinanceUpdateManager;
    use crate::exchange::ExchangeUpdateManager;
    use crate::test_utils::import_local_csv_and_assert;
    use duckdb::DuckdbConnectionManager;
    use r2d2::PooledConnection;
    use std::path::Path;

    fn initial_db() -> PooledConnection<DuckdbConnectionManager> {
        let builder = r2d2::Pool::builder()
            .max_size(2) // 最大连接数
            .min_idle(Some(1)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

        builder
            .build(DuckdbConnectionManager::memory().unwrap())
            .unwrap()
            .get()
            .unwrap()
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
        import_local_csv_and_assert(&conn, "spot_kline", csv_path.as_path(), 3).unwrap();

        let manager = BinanceUpdateManager { connection: conn };
        manager.refresh_spot_kline().await;
    }
}

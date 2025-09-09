use crate::binance::binance_consts::QUERY_LATEST_SQL;
use crate::exchange::KlineUpDate;
use duckdb::DuckdbConnectionManager;
use r2d2::PooledConnection;
use yue::binance::spots::KlineFetcher;

pub struct SpotKlineRefresh<F>
where
    F: KlineFetcher + Send + Sync,
{
    connection: PooledConnection<DuckdbConnectionManager>,
    kline_fetcher: F,
}

impl<F> SpotKlineRefresh<F>
where
    F: KlineFetcher + Send + Sync,
{
    pub fn new(connection: PooledConnection<DuckdbConnectionManager>) -> Self
    where
        F: KlineFetcher + Send + Sync + Default,
    {
        SpotKlineRefresh {
            connection,
            kline_fetcher: F::default(),
        }
    }

    pub fn with_fetcher(
        connection: PooledConnection<DuckdbConnectionManager>,
        kline_fetcher: F,
    ) -> Self {
        SpotKlineRefresh {
            connection,
            kline_fetcher,
        }
    }
}

impl<F> KlineUpDate for SpotKlineRefresh<F>
where
    F: KlineFetcher + Send + Sync,
{
    async fn update(&self) -> String {
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
    use crate::binance::update_manager::SpotKlineRefresh;
    use crate::exchange::KlineUpDate;
    use crate::test_utils::import_local_csv_and_assert;
    use duckdb::DuckdbConnectionManager;
    use mockall::mock;
    use r2d2::PooledConnection;
    use std::path::Path;
    use yue::binance::spots::KlineFetcher;

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

    mock! {
        pub KlineFetcher {}

        #[async_trait::async_trait(?Send)]
        impl KlineFetcher for KlineFetcher {
            async fn get_all_kline_data(
                &self,
                symbol: &str,
                interval: yue::binance::spots::KlineInterval,
                start_time: Option<u64>,
            ) -> Result<(Vec<yue::binance::spots::Kline>, usize), yue::errors::YueError>;
        }
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

        use mockall::predicate::{always, eq};

        let mut mock = MockKlineFetcher::new();

        let manager = SpotKlineRefresh::with_fetcher(conn, mock);
        manager.update().await;
    }
}

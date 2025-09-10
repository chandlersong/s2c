use crate::binance::binance_consts::QUERY_LATEST_SQL;
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::KlineUpdate;

pub struct SpotKlineRefresh<'a> {
    provider: &'a DBProvider,
}

impl<'a> SpotKlineRefresh<'a> {
    pub fn new(provider: &'a DBProvider) -> Self {
        SpotKlineRefresh { provider }
    }
}

impl<'a> KlineUpdate for SpotKlineRefresh<'a> {
    async fn update(&self) -> Result<(), MingLuanError> {
        let conn = self.provider.acquire()?;
        let mut stmt = conn.prepare(QUERY_LATEST_SQL)?;
        let latest_symbol = stmt.query_map([], |row| {
            let symbol: String = row.get(0)?;
            let latest: i64 = row.get(1)?;
            Ok((symbol, latest))
        })?;

        // 打印查询到的每个 symbol 和对应的 latest 时间戳，便于调试
        for entry in latest_symbol {
            let (symbol, timestamp) = entry?;
            println!("latest_symbol -> symbol: {}, latest: {}", symbol, timestamp);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::binance_consts::BinanceTables::SpotKline;
    use crate::binance::klines::SpotKlineRefresh;
    use crate::duck_db::DBProvider;
    use crate::exchange::KlineUpdate;
    use crate::test_utils::import_local_csv_and_assert;
    use duckdb::DuckdbConnectionManager;
    use r2d2::Pool;
    use std::path::Path;

    fn initial_db() -> Pool<DuckdbConnectionManager> {
        let builder = r2d2::Pool::builder()
            .max_size(2) // 最大连接数
            .min_idle(Some(1)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

        builder
            .build(DuckdbConnectionManager::memory().unwrap())
            .unwrap()
    }

    // mock! {
    //     pub KlineFetcher {}
    //
    //     #[async_trait]
    //     impl KlineFetcher for KlineFetcher {
    //         async fn get_all_kline_data(
    //             &self,
    //             symbol: &str,
    //             interval: yue::binance::spots::KlineInterval,
    //             start_time: Option<u64>,
    //         ) -> Result<(Vec<yue::binance::spots::Kline>, usize), yue::errors::YueError>;
    //     }
    // }

    #[tokio::test]
    async fn test_refresh_spot_kline_normal() {
        // 初始化内存数据库连接并建表
        let pool = initial_db();
        let db_provider = DBProvider::new(pool);
        let conn = db_provider.acquire().unwrap();
        conn.execute(SpotKline.create_table_statement().as_str(), [])
            .unwrap();
        // 直接从仓库中的本地 CSV 导入并断言行数为 2
        let csv_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/test_refresh_spot_kline_normal.csv");
        import_local_csv_and_assert(
            &conn,
            SpotKline.table_name().as_str(),
            csv_path.as_path(),
            3,
        )
        .unwrap();

        // use mockall::predicate::{always, eq};
        //
        // let mut mock = MockKlineFetcher::new();

        let manager = SpotKlineRefresh::new(&db_provider);
        manager.update().await;
    }
}

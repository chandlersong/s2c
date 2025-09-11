use crate::binance::binance_consts::QUERY_LATEST_SQL;
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::KlineUpdate;
use crate::utils::get_snowflake_generator;
use snowflake::SnowflakeIdGenerator;
use yue::binance::spots::KlineFetcher;

#[derive(Debug, Clone)]
pub struct KlineData {
    pub id: i64,
    pub symbol: String,
    pub candle_begin_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub quote_volume: f64,
    pub number_of_trades: i64,
    pub taker_buy_base_asset_volume: f64,
    pub taker_buy_quote_asset_volume: f64,
    pub close_time: i64,
}

impl<'a> From<&duckdb::Row<'a>> for KlineData {
    fn from(row: &duckdb::Row) -> Self {
        KlineData {
            id: row.get(0).unwrap_or_default(),
            symbol: row.get(1).unwrap_or_default(),
            candle_begin_time: row.get(2).unwrap_or_default(),
            open: row.get(3).unwrap_or_default(),
            high: row.get(4).unwrap_or_default(),
            low: row.get(5).unwrap_or_default(),
            close: row.get(6).unwrap_or_default(),
            volume: row.get(7).unwrap_or_default(),
            quote_volume: row.get(8).unwrap_or_default(),
            number_of_trades: row.get(9).unwrap_or_default(),
            taker_buy_base_asset_volume: row.get(10).unwrap_or_default(),
            taker_buy_quote_asset_volume: row.get(11).unwrap_or_default(),
            close_time: row.get(12).unwrap_or_default(),
        }
    }
}

impl KlineData {
    pub fn from_binance_kline(symbol: &str, kline: &yue::binance::bn_models::BinanceKline) -> Self {
        let id = get_snowflake_generator()
            .lock()
            .unwrap()
            .real_time_generate();
        KlineData {
            id,
            symbol: symbol.to_string(),
            candle_begin_time: kline.open_time as i64,
            open: kline.open,
            high: kline.high,
            low: kline.low,
            close: kline.close,
            volume: kline.volume,
            quote_volume: kline.quote_asset_volume,
            number_of_trades: kline.number_of_trades as i64,
            taker_buy_base_asset_volume: kline.taker_buy_base_asset_volume,
            taker_buy_quote_asset_volume: kline.taker_buy_quote_asset_volume,
            close_time: kline.close_time as i64,
        }
    }
}

impl std::fmt::Display for KlineData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "KlineData {{ id: {}, symbol: {}, candle_begin_time: {}, open: {}, high: {}, low: {}, close: {}, volume: {}, quote_volume: {}, number_of_trades: {}, taker_buy_base_asset_volume: {}, taker_buy_quote_asset_volume: {}, close_time: {} }}",
            self.id,
            self.symbol,
            self.candle_begin_time,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.quote_volume,
            self.number_of_trades,
            self.taker_buy_base_asset_volume,
            self.taker_buy_quote_asset_volume,
            self.close_time
        )
    }
}

pub struct SpotKlineRefresh<'a, T: KlineFetcher> {
    provider: &'a DBProvider,
    kline_fetcher: &'a T,
    table_name: String,
}

impl<'a, T: KlineFetcher> SpotKlineRefresh<'a, T> {
    pub fn new(provider: &'a DBProvider, kline_fetcher: &'a T, table_name: String) -> Self {
        SpotKlineRefresh {
            provider,
            kline_fetcher,
            table_name,
        }
    }
}

impl<'a, T: KlineFetcher> KlineUpdate for SpotKlineRefresh<'a, T> {
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
    use crate::binance::binance_consts::{ONE_HOUR_MS, QUERY_LATEST_SQL};
    use crate::binance::klines::{KlineData, SpotKlineRefresh};
    use crate::duck_db::DBProvider;
    use crate::errors::MingLuanError;
    use crate::exchange::KlineUpdate;
    use crate::test_utils::{
        TEST_BEGIN_TIMESTAMP, generate_test_kline_vec, import_local_csv_and_assert,
    };
    use async_trait::async_trait;
    use duckdb::DuckdbConnectionManager;
    use mockall::{mock, predicate};
    use r2d2::Pool;
    use std::path::Path;
    use yue::binance::bn_models::BinanceKline;
    use yue::binance::spots::{KlineFetcher, KlineInterval};
    use yue::errors::YueError;

    fn initial_db() -> Pool<DuckdbConnectionManager> {
        let builder = r2d2::Pool::builder()
            .max_size(2) // 最大连接数
            .min_idle(Some(1)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

        builder
            .build(DuckdbConnectionManager::memory().unwrap())
            .unwrap()
    }

    mock! {
        pub KlineFetcher {}

        #[async_trait]
        impl KlineFetcher for KlineFetcher {
               async fn get_all_kline_data(
                                    &self,
                                    symbol: &str,
                                    interval: KlineInterval,
                                    start_time: Option<u64>,
                                ) -> Result<(Vec<BinanceKline>, u16), YueError>;
        }
    }

    #[tokio::test]
    async fn test_refresh_spot_kline_normal() -> Result<(), MingLuanError> {
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

        let mut kline_fetcher = MockKlineFetcher::new();
        kline_fetcher
            .expect_get_all_kline_data()
            .with(
                predicate::eq("BTCUSDT"),
                predicate::eq(KlineInterval::OneHour),
                predicate::always(),
            )
            .returning(|_, _, _| {
                // 返回模拟数据
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });

        kline_fetcher
            .expect_get_all_kline_data()
            .with(
                predicate::eq("ETHUSDT"),
                predicate::eq(KlineInterval::OneHour),
                predicate::always(),
            )
            .returning(|_, _, _| {
                // 返回模拟数据
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });

        let expected = kline_fetcher
            .get_all_kline_data("BTCUSDT", KlineInterval::OneHour, None)
            .await
            .unwrap();
        let manager = SpotKlineRefresh::new(&db_provider, &kline_fetcher, SpotKline.table_name());
        manager.update().await;

        let conn = db_provider.acquire()?;
        let mut stmt =
            conn.prepare(format!("SELECT * FROM {}", SpotKline.table_name()).as_str())?;
        let klines = stmt.query_map([], |row| Ok(KlineData::from(row)))?;

        // 打印查询到的每个 symbol 和对应的 latest 时间戳，便于调试
        for kline in klines {
            match kline {
                Ok(data) => {
                    println!("{}", data);
                }
                Err(e) => {
                    println!("Error parsing row: {}", e);
                }
            }
        }

        Ok(())
    }
}

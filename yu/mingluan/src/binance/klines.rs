use crate::binance::binance_consts::{ONE_HOUR_MS, QUERY_LATEST_SQL};
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::KlineUpdate;
use crate::utils::get_snowflake_generator;
use log::{error, trace};
use yue::binance::spots::{KlineFetcher, KlineInterval};

#[derive(Debug, Clone)]
pub struct KlinePo {
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

impl<'a> From<&duckdb::Row<'a>> for KlinePo {
    fn from(row: &duckdb::Row) -> Self {
        KlinePo {
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

impl KlinePo {
    pub fn from_binance_kline(symbol: &str, kline: &yue::binance::bn_models::BinanceKline) -> Self {
        let id = get_snowflake_generator()
            .lock()
            .unwrap()
            .real_time_generate();
        KlinePo {
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

    pub fn to_insert_sql(&self, table_name: &str) -> String {
        // 简单转义 symbol 字段中的单引号
        let symbol = self.symbol.replace("'", "''");
        format!(
            "INSERT INTO {} (id, symbol, candle_begin_time, open, high, low, close, volume, quote_volume, number_of_trades, taker_buy_base_asset_volume, taker_buy_quote_asset_volume, close_time) VALUES ({}, '{}', {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {});",
            table_name,
            self.id,
            symbol,
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

impl std::fmt::Display for KlinePo {
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
            let latest: u64 = row.get(1)?;
            Ok((symbol, latest))
        })?;

        // 打印查询到的每个 symbol 和对应的 latest 时间戳，便于调试
        for entry in latest_symbol {
            let (symbol, timestamp) = entry?;
            trace!("update -> symbol: {}, latest: {}", symbol, timestamp);

            let fetch_data = self
                .kline_fetcher
                .get_all_kline_data(
                    &symbol,
                    KlineInterval::OneHour,
                    Some(timestamp + ONE_HOUR_MS),
                )
                .await;

            match fetch_data {
                Ok((kline_data, fail_times)) => {
                    let len = kline_data.len();
                    if len <= 1 {
                        ()
                    }
                    //因为币安最后一个都是脏数据，比如说我在11:30获取，他会返回12:00的，但是12:00的还没收盘。所以就默认舍弃
                    let data = &kline_data[..len - 1];
                    trace!(
                        "Fetched {} klines for symbol {}: HTTP status {}",
                        len, symbol, fail_times
                    );

                    for kline in data {
                        let kline_po = KlinePo::from_binance_kline(&symbol, &kline);
                        let insert_sql = kline_po.to_insert_sql(self.table_name.as_str());
                        match conn.execute_batch(&insert_sql) {
                            Ok(_) => {}
                            Err(e) => {
                                error!("Failed to insert kline {}: {}", kline_po, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error fetching klines for symbol {}: {}", symbol, e);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::binance_consts::BinanceTables::SpotKline;
    use crate::binance::binance_consts::ONE_HOUR_MS;
    use crate::binance::klines::{KlinePo, SpotKlineRefresh};
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
        let conn = db_provider.acquire()?;
        conn.execute(SpotKline.create_table_statement().as_str(), [])?;
        // 直接从仓库中的本地 CSV 导入并断言行数为 2
        let csv_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/test_refresh_spot_kline_normal.csv");

        import_local_csv_and_assert(
            &conn,
            SpotKline.table_name().as_str(),
            csv_path.as_path(),
            3,
        )?;

        // use mockall::predicate::{always, eq};
        //

        let mut kline_fetcher = MockKlineFetcher::new();
        kline_fetcher
            .expect_get_all_kline_data()
            .with(
                predicate::eq("BTCUSDT"),
                predicate::eq(KlineInterval::OneHour),
                predicate::eq(Some(1694102400000 + ONE_HOUR_MS)),
            )
            .times(1)
            .returning(|_, _, _| {
                // 返回模拟数据
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });

        kline_fetcher
            .expect_get_all_kline_data()
            .times(1)
            .with(
                predicate::eq("ETHUSDT"),
                predicate::eq(KlineInterval::OneHour),
                predicate::eq(Some(1694101400000 + ONE_HOUR_MS)),
            )
            .returning(|_, _, _| {
                // 返回模拟数据
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });

        let manager = SpotKlineRefresh::new(&db_provider, &kline_fetcher, SpotKline.table_name());
        let res = manager.update().await;

        assert!(res.is_ok());

        let conn = db_provider.acquire()?;
        let mut stmt =
            conn.prepare(format!("SELECT * FROM {}", SpotKline.table_name()).as_str())?;
        let kline_data: Vec<KlinePo> = stmt
            .query_map([], |row| Ok(KlinePo::from(row)))?
            .filter_map(Result::ok)
            .collect();

        assert_eq!(&kline_data.len(), &5); // 原有3条 + 每个symbol新增2条
        // 打印查询到的每个 symbol 和对应的 latest 时间戳，便于调试
        let mut btc_vec: Vec<KlinePo> = vec![];
        let mut eth_vec: Vec<KlinePo> = vec![];
        for kline in kline_data {
            if kline.symbol == "BTCUSDT" {
                btc_vec.push(kline);
            } else if kline.symbol == "ETHUSDT" {
                eth_vec.push(kline);
            }
        }
        assert_eq!(&btc_vec.len(), &3);
        assert_eq!(&eth_vec.len(), &2);

        Ok(())
    }

    #[test]
    fn test_to_insert_sql_basic() {
        let kline = KlinePo {
            id: 123456789,
            symbol: "BTCUSDT".to_string(),
            candle_begin_time: 1694448000000,
            open: 10000.1,
            high: 10100.0,
            low: 9900.0,
            close: 10050.0,
            volume: 123.45,
            quote_volume: 123456.78,
            number_of_trades: 100,
            taker_buy_base_asset_volume: 12.34,
            taker_buy_quote_asset_volume: 1234.56,
            close_time: 1694451600000,
        };
        let sql = kline.to_insert_sql(SpotKline.table_name().as_str());
        println!("Generated SQL: {}", sql);
        assert!(sql.contains("INSERT INTO spot_kline"));
        assert!(sql.contains("'BTCUSDT'"));
        assert!(sql.contains("123456789"));
        assert!(sql.contains("10000.1"));
        assert!(sql.contains("1694451600000"));
    }

    #[test]
    fn test_to_insert_sql_symbol_escape() {
        let kline = KlinePo {
            id: 1,
            symbol: "O'MATIC".to_string(),
            candle_begin_time: 0,
            open: 1.0,
            high: 1.0,
            low: 1.0,
            close: 1.0,
            volume: 1.0,
            quote_volume: 1.0,
            number_of_trades: 1,
            taker_buy_base_asset_volume: 1.0,
            taker_buy_quote_asset_volume: 1.0,
            close_time: 0,
        };
        let sql = kline.to_insert_sql("spot_kline");
        assert!(sql.contains("'O''MATIC'")); // SQL单引号转义
    }

    #[test]
    fn test_display_trait() {
        let kline = KlinePo {
            id: 42,
            symbol: "ETHUSDT".to_string(),
            candle_begin_time: 123,
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 10.0,
            quote_volume: 20.0,
            number_of_trades: 5,
            taker_buy_base_asset_volume: 2.0,
            taker_buy_quote_asset_volume: 4.0,
            close_time: 456,
        };
        let s = format!("{}", kline);
        assert!(s.contains("KlineData"));
        assert!(s.contains("ETHUSDT"));
        assert!(s.contains("42"));
    }
}

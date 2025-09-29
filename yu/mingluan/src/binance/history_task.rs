use crate::actix_jobs::AsyncRepeatTask;
use crate::binance::binance_consts::{BinanceTables, GENESIS_2020_MS, ONE_HOUR_MS};
use crate::binance::bn_dashboard::TradingSymbols;
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::HistoryFetcherFactory;
use crate::utils::get_snowflake_generator;
use async_trait::async_trait;
use duckdb::{appender_params_from_iter, DropBehavior};
use li::tools::time::unix_2_readable;
use log::{debug, error, info};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use yue::binance::bn_models::{BinanceKline, FundingRate, SymbolType, ToQueryParams};
use yue::binance::history_data::{HistoryFetcher, HistoryVo, MuteHistoryParam};

pub trait HistoryPO: Debug {
    type Source: HistoryVo;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self;

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>>;
}

pub trait HistoryDataWriter<O: HistoryPO>: Send + Sync {
    fn write_batch(&self, data: Vec<O>) -> Result<(), MingLuanError>;
    fn query_latest_symbols(&self, trading_symbols: &TradingSymbols) -> Result<Vec<(String, u64)>, MingLuanError>;
}

pub struct DuckDBHistoryDataWriter {
    provider: DBProvider,
    table: BinanceTables,
    symbol_type: SymbolType,
}

impl DuckDBHistoryDataWriter {
    pub fn new(provider: DBProvider, table: BinanceTables, symbol_type: SymbolType) -> Self {
        DuckDBHistoryDataWriter {
            provider,
            table,
            symbol_type,
        }
    }
}

impl<O: HistoryPO> HistoryDataWriter<O> for DuckDBHistoryDataWriter {
    fn write_batch(&self, data: Vec<O>) -> Result<(), MingLuanError> {
        if data.is_empty() {
            return Ok(());
        }
        let mut conn = self.provider.acquire()?;
        let mut tx = conn.transaction()?;
        tx.set_drop_behavior(DropBehavior::Commit);
        let mut appender = match tx.appender(&self.table.table_name()) {
            Ok(a) => a,
            Err(e) => {
                error!("Failed to create appender for table {}: {}", self.table.table_name(), e);
                return Err(MingLuanError::CustomError("Failed to create appender".to_string()));
            }
        };

        for po in data {
            if let Err(e) = appender.append_row(po.to_params()) {
                error!("Failed to append kline {:?}: {}", po, e);
            }
        }
        if let Err(e) = appender.flush() {
            error!("Failed to flush appender for table {}: {}", self.table.table_name(), e);
        };
        Ok(())
    }

    fn query_latest_symbols(&self, trading_symbols: &TradingSymbols) -> Result<Vec<(String, u64)>, MingLuanError> {
        let conn = self.provider.acquire()?;
        let query_sql = match self.table.query_lastest_record() {
            None => Err(MingLuanError::new(&format!(
                "Table {} does not support querying latest record",
                self.table.table_name()
            )))?,
            Some(s) => s,
        };
        let mut stmt = conn.prepare(&query_sql)?;
        let symbol_in_db: HashMap<String, u64> = stmt
            .query_map([], |row| {
                let symbol: String = row.get(0)?;
                let latest: u64 = row.get(1)?;
                Ok((symbol, latest))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect();
        let symbols = match self.symbol_type {
            SymbolType::Spot => &trading_symbols.trading_spot_symbols,
            SymbolType::Swap => &trading_symbols.trading_swap_symbols,
        };

        let filtered: Vec<(String, u64)> = symbols
            .into_iter()
            .filter(|s| s.ends_with("USDT"))
            .map(|s| (s.clone(), symbol_in_db.get(s).copied().unwrap_or(GENESIS_2020_MS)))
            .collect();
        info!("fetched {} trading symbols", filtered.len());
        Ok(filtered)
    }
}

#[derive(Debug, Clone)]
pub struct KlinePo {
    pub id: i64,
    pub symbol: String,
    pub candle_begin_time: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub quote_volume: f64,
    pub number_of_trades: u64,
    pub taker_buy_base_asset_volume: f64,
    pub taker_buy_quote_asset_volume: f64,
    pub close_time: u64,
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

impl HistoryPO for KlinePo {
    type Source = BinanceKline;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self {
        let id = get_snowflake_generator().lock().unwrap().real_time_generate();
        KlinePo {
            id,
            symbol: symbol.expect("Symbol must be provided").to_string(),
            candle_begin_time: source.open_time,
            open: source.open,
            high: source.high,
            low: source.low,
            close: source.close,
            volume: source.volume,
            quote_volume: source.quote_asset_volume,
            number_of_trades: source.number_of_trades,
            taker_buy_base_asset_volume: source.taker_buy_base_asset_volume,
            taker_buy_quote_asset_volume: source.taker_buy_quote_asset_volume,
            close_time: source.close_time,
        }
    }

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.symbol as &dyn duckdb::ToSql,
            &self.candle_begin_time as &dyn duckdb::ToSql,
            &self.open as &dyn duckdb::ToSql,
            &self.high as &dyn duckdb::ToSql,
            &self.low as &dyn duckdb::ToSql,
            &self.close as &dyn duckdb::ToSql,
            &self.volume as &dyn duckdb::ToSql,
            &self.quote_volume as &dyn duckdb::ToSql,
            &self.number_of_trades as &dyn duckdb::ToSql,
            &self.taker_buy_base_asset_volume as &dyn duckdb::ToSql,
            &self.taker_buy_quote_asset_volume as &dyn duckdb::ToSql,
            &self.close_time as &dyn duckdb::ToSql,
        ])
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

#[derive(Debug, Clone)]
pub struct FundingRatePo {
    pub id: i64,
    pub symbol: String,
    pub funding_rate: f64,
    pub funding_time: u64,
    pub mark_price: f64,
}

impl<'a> From<&duckdb::Row<'a>> for FundingRatePo {
    fn from(row: &duckdb::Row) -> Self {
        FundingRatePo {
            id: row.get(0).unwrap_or_default(),
            symbol: row.get(1).unwrap_or_default(),
            funding_rate: row.get(2).unwrap_or_default(),
            funding_time: row.get(3).unwrap_or_default(),
            mark_price: row.get(4).unwrap_or_default(),
        }
    }
}

impl HistoryPO for FundingRatePo {
    type Source = FundingRate;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self {
        let id = get_snowflake_generator().lock().unwrap().real_time_generate();
        FundingRatePo {
            id,
            symbol: symbol.expect("Symbol must be provided").to_string(),
            funding_rate: source.funding_rate,
            funding_time: source.funding_time,
            mark_price: source.mark_price,
        }
    }

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>> {
        appender_params_from_iter(vec![
            &self.id as &dyn duckdb::ToSql,
            &self.symbol as &dyn duckdb::ToSql,
            &self.funding_rate as &dyn duckdb::ToSql,
            &self.funding_time as &dyn duckdb::ToSql,
            &self.mark_price as &dyn duckdb::ToSql,
        ])
    }
}

#[derive(Clone)]
pub struct UpdateHistoryTask<F, P, R, V>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync,
    V: HistoryVo + Clone,
    R: HistoryPO + Clone,
{
    kline_fetcher_factory: F,
    spot_info: Arc<RwLock<TradingSymbols>>,
    data_writer: Arc<dyn HistoryDataWriter<R> + Send + Sync>,
}

impl<F, P, R, V> UpdateHistoryTask<F, P, R, V>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync,
    V: HistoryVo + Clone,
    R: HistoryPO + Clone,
{
    pub fn new(factory: F, spot_info: Arc<RwLock<TradingSymbols>>, data_writer: Arc<dyn HistoryDataWriter<R> + Send + Sync>) -> Self {
        UpdateHistoryTask {
            kline_fetcher_factory: factory,
            spot_info,
            data_writer,
        }
    }

    async fn fetch_symbol_data<T>(kline_fetcher: T, param: P, timestamp: u64, tx: mpsc::Sender<Result<Vec<R>, yue::errors::YueError>>)
    where
        T: HistoryFetcher<P, V> + Send + Sync + 'static,
        R: HistoryPO<Source = V> + Clone,
    {
        debug!("update -> symbol: {}, latest: {}", param.get_symbol(), unix_2_readable(&timestamp));
        let result = match kline_fetcher.get_all_kline_data(param.clone(), Some(timestamp + ONE_HOUR_MS)).await {
            Ok((kline_data, fail_times)) => {
                let len = kline_data.len();
                if len <= 1 {
                    Ok(Vec::new())
                } else {
                    let data = &kline_data[..len - 1];
                    debug!("Fetched {} klines for symbol {}: fail times {}", len, param.get_symbol(), fail_times);
                    // 写入数据库
                    let kline_pos: Vec<R> = data.iter().map(|kline| R::from_source(Some(param.get_symbol()), kline)).collect();
                    Ok(kline_pos)
                }
            }
            Err(e) => {
                error!("Error fetching klines for symbol {}: {}", param.get_symbol(), e);
                Err(e)
            }
        };
        if tx.send(result).await.is_err() {
            error!("Failed to send result for symbol {}", param.get_symbol());
        }
    }
}

#[async_trait]
impl<F, P, R, V> AsyncRepeatTask for UpdateHistoryTask<F, P, R, V>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + Unpin + 'static,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + Clone + 'static,
    R: HistoryPO<Source = V> + Send + Sync + Clone + 'static,
{
    async fn execute(&self) -> Result<(), MingLuanError> {
        let latest_symbol = self.data_writer.query_latest_symbols(&self.spot_info.read().unwrap())?;
        let (tx, mut rx) = mpsc::channel(100);
        let symbol_count = latest_symbol.len();

        for (symbol, timestamp) in latest_symbol {
            let tx_clone = tx.clone();
            let kline_fetcher = self.kline_fetcher_factory.create_fetcher();
            // 构造 Param
            let param = P::initial(symbol, 1000, yue::binance::history_data::HistoryInterval::OneHour);
            tokio::spawn({
                let tx_clone = tx_clone.clone();
                let param = param.clone();
                let kline_fetcher = kline_fetcher;
                async move {
                    Self::fetch_symbol_data::<_>(kline_fetcher, param, timestamp, tx_clone).await;
                }
            });
        }
        drop(tx);

        // 在主线程中接收结果并串行插入数据库
        for _ in 0..symbol_count {
            if let Some(result) = rx.recv().await {
                match result {
                    Ok(data) => {
                        let _ = self.data_writer.write_batch(data);
                    }
                    Err(e) => {
                        error!("Failed to fetch symbol data: {}", e);
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::actix_jobs::AsyncRepeatTask;
    use crate::binance::binance_consts::BinanceTables::SpotKline;
    use crate::binance::binance_consts::ONE_HOUR_MS;
    use crate::binance::bn_dashboard::TradingSymbols;
    use crate::binance::history_task::{DuckDBHistoryDataWriter, KlinePo, UpdateHistoryTask};
    use crate::duck_db::DBProvider;
    use crate::errors::MingLuanError;
    use crate::exchange::HistoryFetcherFactory;
    use crate::test_utils::{generate_test_kline_vec, import_local_csv_and_assert, TEST_BEGIN_TIMESTAMP};
    use async_trait::async_trait;
    use duckdb::DuckdbConnectionManager;
    use mockall::{mock, predicate};
    use r2d2::Pool;
    use std::path::Path;
    use std::sync::{Arc, RwLock};
    use yue::binance::bn_models::{BinanceKline, SymbolType};
    use yue::binance::history_data::{HistoryFetcher, HistoryInterval, KlineParams, MuteHistoryParam};
    use yue::errors::YueError;

    fn initial_db() -> Pool<DuckdbConnectionManager> {
        let builder = Pool::builder()
            .max_size(2) // 最大连接数
            .min_idle(Some(1)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

        builder.build(DuckdbConnectionManager::memory().unwrap()).unwrap()
    }

    // mock 测试部分同步修正
    mock! {
        pub HistoryFetcher {}

        impl Clone for HistoryFetcher {
            fn clone(&self) -> Self {
                HistoryFetcher {}
            }
        }

        #[async_trait]
        impl HistoryFetcher<KlineParams, BinanceKline> for HistoryFetcher {
            async fn get_all_kline_data(
                &self,
                param: KlineParams,
                start_time: Option<u64>,
            ) -> Result<(Vec<BinanceKline>, u16), YueError>;
        }
    }

    #[derive(Clone)]
    struct MockHistoryFetcherFactory {}

    impl HistoryFetcherFactory for MockHistoryFetcherFactory {
        type Param = KlineParams;
        type Output = BinanceKline;
        type Fetcher = MockHistoryFetcher;
        fn create_fetcher(&self) -> Self::Fetcher {
            create_mock_history_fetch_for_test_refresh_spot_kline_normal()
        }
    }

    fn create_mock_history_fetch_for_test_refresh_spot_kline_normal() -> MockHistoryFetcher {
        let mut fetcher = MockHistoryFetcher::new();
        let btc_param = KlineParams::initial("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let eth_param = KlineParams::initial("ETHUSDT".to_string(), 1000, HistoryInterval::OneHour);
        fetcher
            .expect_get_all_kline_data()
            .with(predicate::eq(btc_param.clone()), predicate::eq(Some(1694102300000 + ONE_HOUR_MS)))
            .returning(|_, _| {
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });
        fetcher
            .expect_get_all_kline_data()
            .with(predicate::eq(eth_param.clone()), predicate::eq(Some(1694101300000 + ONE_HOUR_MS)))
            .returning(|_, _| {
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });
        fetcher
    }

    #[tokio::test]
    async fn test_refresh_spot_kline_normal() -> Result<(), MingLuanError> {
        // 初始化内存数据库连接并建表
        let pool = initial_db();
        let db_provider = DBProvider::new(pool);
        let conn = db_provider.acquire()?;
        conn.execute(SpotKline.create_table_statement().as_str(), [])?;
        // 直接从仓库中的本地 CSV 导入并断言行数为 2
        let csv_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/test_refresh_spot_kline_normal.csv");

        let trading_symbols = vec!["BTCUSDT".to_string()];
        let spot_info = Arc::new(RwLock::new(TradingSymbols {
            trading_spot_symbols: trading_symbols,
            trading_swap_symbols: vec![],
        }));
        import_local_csv_and_assert(&conn, SpotKline.table_name().as_str(), csv_path.as_path(), 7)?;

        let factory = MockHistoryFetcherFactory {};
        let data_writer = Arc::new(DuckDBHistoryDataWriter::new(db_provider.clone(), SpotKline, SymbolType::Spot));
        let manager: UpdateHistoryTask<MockHistoryFetcherFactory, KlineParams, KlinePo, BinanceKline> =
            UpdateHistoryTask::new(factory, spot_info, data_writer);
        let res = manager.execute().await;

        println!("{:?}", res);
        assert!(res.is_ok());
        let conn = db_provider.acquire()?;
        let mut stmt = conn.prepare(format!("SELECT * FROM {}", SpotKline.table_name()).as_str())?;
        let kline_data: Vec<KlinePo> = stmt.query_map([], |row| Ok(KlinePo::from(row)))?.filter_map(Result::ok).collect();

        assert_eq!(&kline_data.len(), &8); // 原有3条 + 每个symbol新增2条
        let mut btc_vec: Vec<KlinePo> = vec![];
        let mut eth_vec: Vec<KlinePo> = vec![];
        for kline in kline_data {
            if kline.symbol == "BTCUSDT" {
                btc_vec.push(kline);
            } else if kline.symbol == "ETHUSDT" {
                eth_vec.push(kline);
            }
        }

        assert_eq!(&btc_vec.len(), &6);
        let insert_btc = btc_vec.last().unwrap();
        assert_eq!(insert_btc.candle_begin_time, 1609459200000);
        assert_eq!(insert_btc.open, 10000.0);
        assert_eq!(insert_btc.high, 10100.0);
        assert_eq!(insert_btc.low, 9900.0);
        assert_eq!(insert_btc.close, 1.0);
        assert_eq!(insert_btc.volume, 10.0);
        assert_eq!(insert_btc.quote_volume, 100500.0);
        assert_eq!(insert_btc.number_of_trades, 100);
        assert_eq!(insert_btc.taker_buy_base_asset_volume, 5.0);
        assert_eq!(insert_btc.taker_buy_quote_asset_volume, 50000.0);
        assert_eq!(insert_btc.close_time, 1609462799999);

        assert_eq!(&eth_vec.len(), &2);

        Ok(())
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

use crate::binance::binance_db_consts::BinanceTables;
use crate::binance::bn_dashboard::TradingSymbol;
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use crate::exchange::{ExchangeDashBoard, HistoryFetcherFactory};
use crate::utils::get_snowflake_generator;
use async_trait::async_trait;
use duckdb::{appender_params_from_iter, DropBehavior};
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc, GENESIS_2020_MS, ONE_HOUR_MS};
use log::{debug, error, info};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;
use yue::binance::bn_models::common::{HistoryVo, SymbolType, ToQueryParams};
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::history_data::{HistoryFetcher, MuteHistoryParam};

pub trait HistoryPO: Debug {
    type Source: HistoryVo;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self;

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>>;
}

pub trait HistoryDataWriter<O: HistoryPO, D: ExchangeDashBoard<TradingSymbol = TradingSymbol>>: Send + Sync {
    fn write_batch(&self, data: Vec<O>) -> Result<(), YuError>;
    fn query_latest_symbols(&self, dash_board: Arc<D>, now: u64) -> Result<Vec<(String, u64)>, YuError>;
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

impl<O: HistoryPO, D: ExchangeDashBoard<TradingSymbol = TradingSymbol>> HistoryDataWriter<O, D> for DuckDBHistoryDataWriter {
    fn write_batch(&self, data: Vec<O>) -> Result<(), YuError> {
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
                return Err(YuError::CustomError("Failed to create appender".to_string()));
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

    fn query_latest_symbols(&self, dashboard: Arc<D>, now: u64) -> Result<Vec<(String, u64)>, YuError> {
        let conn = self.provider.acquire()?;
        let query_sql = match self.table.query_lastest_record() {
            None => Err(YuError::new(&format!(
                "Table {} does not support querying latest record",
                self.table.table_name()
            )))?,
            Some(s) => s,
        };
        let mut stmt = conn.prepare(&query_sql)?;
        let symbol_in_db: HashMap<String, u64> = stmt
            .query_map([], |row| {
                let symbol: String = row.get(0)?;
                let close: u64 = row.get(1)?;
                Ok((symbol, close))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect();
        let symbols = match self.symbol_type {
            SymbolType::Spot => dashboard.spot_symbols().read().unwrap().clone(),
            SymbolType::Swap => dashboard.swap_symbols().read().unwrap().clone(),
            _ => {
                panic!("Not support symbol type");
            }
        };

        let filtered: Vec<(String, u64)> = symbols
            .into_iter()
            .filter(|s| s.quote_asset.eq("USDT"))
            .map(|s| {
                (
                    s.symbol.clone(),
                    symbol_in_db
                        .get(&s.symbol)
                        .copied()
                        .unwrap_or(s.on_board_time.unwrap_or_else(|| GENESIS_2020_MS)),
                )
            })
            .filter(|r| now - r.1 > ONE_HOUR_MS)
            .collect();
        info!("{}:fetched {} trading symbols", self.table.table_name(), filtered.len());
        Ok(filtered)
    }
}

#[derive(Debug, Clone)]
pub struct FundingRatePo {
    pub id: i64,
    pub symbol: String,
    pub funding_rate: f64,
    pub funding_time: u64,
    pub mark_price: Option<f64>,
}

impl<'a> From<&duckdb::Row<'a>> for FundingRatePo {
    fn from(row: &duckdb::Row) -> Self {
        FundingRatePo {
            id: row.get(0).unwrap_or_default(),
            symbol: row.get(1).unwrap_or_default(),
            funding_rate: row.get(2).unwrap_or_default(),
            funding_time: row.get(3).unwrap_or_default(),
            mark_price: row.get(4).ok(), // 支持数据库字段为空
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

/// 初始化的历史数据任务，每次启动的时候，都会调用
/// NEXT：写一个实时更新的task
#[derive(Clone)]
pub struct InitialHistoryTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync,
    V: HistoryVo + Clone,
    R: HistoryPO + Clone,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync,
{
    kline_fetcher_factory: F,
    exchange_dashboard: Arc<D>,
    data_writer: Arc<dyn HistoryDataWriter<R, D> + Send + Sync>,
    task_name: String,
}

impl<F, P, R, V, D> InitialHistoryTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync,
    V: HistoryVo + Clone,
    R: HistoryPO + Clone,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync,
{
    pub fn new(factory: F, exchange_dashboard: Arc<D>, data_writer: Arc<dyn HistoryDataWriter<R, D> + Send + Sync>, task_name: String) -> Self {
        InitialHistoryTask {
            kline_fetcher_factory: factory,
            exchange_dashboard,
            data_writer,
            task_name,
        }
    }

    pub async fn fetch_symbol_data<T>(
        kline_fetcher: T,
        param: P,
        timestamp: u64,
        tx: mpsc::Sender<Result<Vec<R>, yue::errors::YueError>>,
        task_name: &str,
    ) where
        T: HistoryFetcher<P, V> + Send + Sync + 'static,
        R: HistoryPO<Source = V> + Clone,
    {
        debug!(
            "update {} -> symbol: {}, latest: {}",
            task_name,
            param.get_symbol(),
            unix_2_readable(&timestamp)
        );
        let result = match kline_fetcher.get_all_kline_data(param.clone(), Some(timestamp)).await {
            Ok((kline_data, fail_times)) => {
                let len = kline_data.len();
                if len <= 1 {
                    Ok(Vec::new())
                } else {
                    let data = &kline_data[..len - 1];
                    debug!(
                        "{}:Fetched {} klines for symbol {}: fail times {}",
                        task_name,
                        len,
                        param.get_symbol(),
                        fail_times
                    );
                    // 写入数据库
                    let kline_pos: Vec<R> = data.iter().map(|kline| R::from_source(Some(param.get_symbol()), kline)).collect();
                    Ok(kline_pos)
                }
            }
            Err(e) => {
                error!("{}:Error fetching klines for symbol {}: {}", task_name, param.get_symbol(), e);
                Err(e)
            }
        };
        if tx.send(result).await.is_err() {
            error!("{}:Failed to send result for symbol {}", task_name, param.get_symbol());
        }
    }
}

#[async_trait]
impl<F, P, R, V, D> AsyncRepeatTask for InitialHistoryTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + Unpin + 'static,
    P: MuteHistoryParam + ToQueryParams + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + Clone + 'static,
    R: HistoryPO<Source = V> + Send + Sync + Clone + 'static,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync + Clone + 'static,
{
    async fn execute(&self) -> Result<(), LiError> {
        let now = unix_time_now_u64_utc();
        let latest_symbol = self
            .data_writer
            .query_latest_symbols(self.exchange_dashboard.clone(), now)
            .map_err(|e| LiError::CustomError(format!("query error: {}", e)))?;
        let (tx, mut rx) = mpsc::channel(100);
        let symbol_count = latest_symbol.len();

        info!("start fetch {},symbol:{}", self.task_name, symbol_count);

        for (symbol, timestamp) in latest_symbol {
            let tx_clone = tx.clone();
            let kline_fetcher = self.kline_fetcher_factory.create_fetcher();
            // NEXT： 这里1000变成参数化，现在是历史数据无所谓。但是实盘需要准确一点
            let param = P::initial(symbol, 1000, yue::binance::history_data::HistoryInterval::OneHour);
            let task_name = self.task_name().to_string();
            tokio::spawn({
                let tx_clone = tx_clone.clone();
                let param = param.clone();
                let kline_fetcher = kline_fetcher;
                async move {
                    Self::fetch_symbol_data::<_>(kline_fetcher, param, timestamp, tx_clone, &task_name).await;
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
        info!("finish fetch {}", self.task_name);
        Ok(())
    }

    fn task_name(&self) -> &str {
        &self.task_name
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::binance_db_consts::BinanceTables::SpotKline;
    use crate::binance::bn_dashboard::{BinanceDashboard, TradingSymbol};
    use crate::binance::history_task::{DuckDBHistoryDataWriter, InitialHistoryTask};
    use crate::binance::models::po::KlinePo;
    use crate::duck_db::DBProvider;
    use crate::errors::YuError;
    use crate::exchange::HistoryFetcherFactory;
    use crate::test_utils::{generate_test_kline_vec, import_local_csv_and_assert, TEST_BEGIN_TIMESTAMP};
    use crate::utils::initial_memory_db;
    use async_trait::async_trait;
    use li::actix_jobs::AsyncRepeatTask;
    use li::errors::LiError;
    use li::tools::time::ONE_HOUR_MS;
    use mockall::{mock, predicate};
    use std::path::Path;
    use std::sync::Arc;
    use yue::binance::bn_models::common::SymbolType;
    use yue::binance::bn_models::spot_restful::BinanceKline;
    use yue::binance::history_data::{CommonParam, HistoryFetcher, HistoryInterval, MuteHistoryParam};
    use yue::errors::YueError;

    // mock 测试部分同步修正
    mock! {
        pub HistoryFetcher {}

        impl Clone for HistoryFetcher {
            fn clone(&self) -> Self {
                HistoryFetcher {}
            }
        }

        #[async_trait]
        impl HistoryFetcher<CommonParam, BinanceKline> for HistoryFetcher {
            async fn get_all_kline_data(
                &self,
                param: CommonParam,
                start_time: Option<u64>,
            ) -> Result<(Vec<BinanceKline>, u16), YueError>;
        }
    }

    #[derive(Clone)]
    struct MockHistoryFetcherFactory {}

    impl HistoryFetcherFactory for MockHistoryFetcherFactory {
        type Param = CommonParam;
        type Output = BinanceKline;
        type Fetcher = MockHistoryFetcher;
        fn create_fetcher(&self) -> Self::Fetcher {
            create_mock_history_fetch_for_test_refresh_spot_kline_normal()
        }
    }

    fn create_mock_history_fetch_for_test_refresh_spot_kline_normal() -> MockHistoryFetcher {
        let mut fetcher = MockHistoryFetcher::new();
        let btc_param = CommonParam::initial("BTCUSDT".to_string(), 1000, HistoryInterval::OneHour);
        let eth_param = CommonParam::initial("ETHUSDT".to_string(), 1000, HistoryInterval::OneHour);
        fetcher
            .expect_get_all_kline_data()
            .with(predicate::eq(btc_param.clone()), predicate::eq(Some(1694102460000)))
            .returning(|_, _| {
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });
        fetcher
            .expect_get_all_kline_data()
            .with(predicate::eq(eth_param.clone()), predicate::eq(Some(1694101300000)))
            .returning(|_, _| {
                let klines = generate_test_kline_vec(TEST_BEGIN_TIMESTAMP, ONE_HOUR_MS, 1.0, 2);
                Ok((klines, 200))
            });
        fetcher
    }

    #[tokio::test]
    async fn test_refresh_spot_kline_normal() -> Result<(), YuError> {
        // 初始化内存数据库连接并建表
        let pool = initial_memory_db();
        let db_provider = DBProvider::new(pool);
        let conn = db_provider.acquire()?;
        let binding = SpotKline.create_table_statement();
        let table_initial_stmt = binding.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                conn.execute(sql, [])?;
            }
        }
        // 直接从仓库中的本地 CSV 导入并断言行数为 2
        let csv_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/test_refresh_spot_kline_normal.csv");

        let trading_symbols = vec![TradingSymbol {
            symbol: "BTCUSDT".to_string(),
            on_board_time: None,
            quote_asset: "USDT".to_string(),
            status: "TRADING".to_string(),
        }];
        let dash_board = Arc::new(BinanceDashboard::new_with_data(trading_symbols, vec![]));
        import_local_csv_and_assert(&conn, SpotKline.table_name().as_str(), csv_path.as_path(), 7)?;

        let factory = MockHistoryFetcherFactory {};
        let data_writer = Arc::new(DuckDBHistoryDataWriter::new(db_provider.clone(), SpotKline, SymbolType::Spot));
        let manager: InitialHistoryTask<MockHistoryFetcherFactory, CommonParam, KlinePo, BinanceKline, BinanceDashboard> =
            InitialHistoryTask::new(factory, dash_board, data_writer, "test_refresh_spot_kline_normal".to_string());
        let res: Result<(), LiError> = manager.execute().await;

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
            interval: 0,
            first_trade_id: None,
            last_trade_id: None,
        };
        let s = format!("{}", kline);
        assert!(s.contains("KlineData"));
        assert!(s.contains("ETHUSDT"));
        assert!(s.contains("42"));
    }
}

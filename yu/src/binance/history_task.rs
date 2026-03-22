use crate::binance::binance_db_consts::BinanceTables;
use crate::binance::bn_dashboard::TradingSymbol;
use crate::duck_db::DBProvider;
use crate::errors::YuError;
use crate::exchange::{ExchangeDashBoard, HistoryFetcherFactory};
use async_trait::async_trait;
use duckdb::DropBehavior;
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use li::tools::time::{unix_2_readable, UnixTimeStamp};
use log::{debug, error, info};
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::mpsc;
use yue::binance::bn_models::common::{HistoryVo, SymbolType, ToRequestBuilder};
use yue::binance::history_data::{HistoryFetcher, MuteHistoryParam};
use yue::errors::YueError;
use yue::models::HistoryInterval;

pub trait HistoryPO: Debug {
    type Source: HistoryVo;

    fn from_source(symbol: Option<&str>, source: &Self::Source) -> Self;

    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>>;
}

pub trait HistoryDataWriter<O: HistoryPO, D: ExchangeDashBoard<TradingSymbol = TradingSymbol>>: Send + Sync {
    ///
    /// 批量写入历史数据
    ///
    fn write_batch(&self, data: Vec<O>) -> Result<(), YuError>;

    ///
    /// 数据库为是否为空
    ///
    fn is_empty(&self) -> Result<bool, YuError>;
}

pub struct DuckDBHistoryDataWriter {
    provider: DBProvider,
    table: BinanceTables,
}

impl DuckDBHistoryDataWriter {
    pub fn new(provider: DBProvider, table: BinanceTables) -> Self {
        DuckDBHistoryDataWriter { provider, table }
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

    ///
    /// 这里只是判断当前表是否有没有记录，没有记录，就表示没有记录
    ///
    /// FUTURE: 改进判断数据库为空的方式。
    /// 因为现有方式还是太简单。但是考虑到初始化的复杂程度，其实暂缓开发。
    /// 可以参考一下其他数据中心的写法。
    ///
    /// 步骤，
    /// 1. 通过sql语句，判断其当前表是否为空，如果为空，则返回true
    /// 2. 只要有数据，就返回false
    ///
    ///
    fn is_empty(&self) -> Result<bool, YuError> {
        // 如果表没有提供 query_lastest_record SQL，则认为没有可查询的最新记录
        let query_sql_opt = self.table.count_records();
        if query_sql_opt.is_none() {
            return Err(YuError::CustomError(format!(
                "Table {:?} does not support counting records",
                self.table.table_name()
            )));
        }

        let sql = query_sql_opt.unwrap();
        let conn = self.provider.acquire()?;

        let mut stmt = conn.prepare(sql.as_str())?;
        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let count: i64 = row.get(0)?;
            return if count == 0 { Ok(true) } else { Ok(false) };
        }

        Err(YuError::CustomError(format!("Table {:?} 数据库访问失败", self.table.table_name())))
    }
}

/// 初始化的历史数据任务，每次启动的时候，都会调用
/// NEXT：写一个实时更新的task
#[derive(Clone)]
pub struct HistoryDataTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync,
    V: HistoryVo + Clone,
    R: HistoryPO + Clone,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync + 'static,
{
    kline_fetcher_factory: F,
    exchange_dashboard: Arc<D>,
    data_writer: Arc<dyn HistoryDataWriter<R, D> + Send + Sync>,
    task_name: String,
    symbol_type: SymbolType,
    interval: HistoryInterval,
}

impl<F, P, R, V, D> HistoryDataTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + 'static,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + 'static,
    R: HistoryPO<Source = V> + Clone + Send + Sync + 'static,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync,
{
    pub fn new(
        factory: F,
        exchange_dashboard: Arc<D>,
        data_writer: Arc<dyn HistoryDataWriter<R, D> + Send + Sync>,
        task_name: String,
        symbol_type: SymbolType,
        interval: Option<HistoryInterval>,
    ) -> Self {
        let actual_interval = interval.unwrap_or_else(|| HistoryInterval::FiveMinutes);
        HistoryDataTask {
            kline_fetcher_factory: factory,
            exchange_dashboard,
            data_writer,
            task_name,
            symbol_type,
            interval: actual_interval,
        }
    }

    ///
    /// 并发获取所有交易对的历史数据并串行写入数据库
    ///
    /// # 参数
    /// * `symbols` - 需要处理的交易对列表
    /// * `earliest_time` - 数据获取的开始时间
    /// * `now_timestamp` - 当前时间戳
    ///
    async fn fetch_and_write_history_data(
        &self,
        symbols: Vec<TradingSymbol>,
        start_timestamp: UnixTimeStamp,
        end_timestamp: UnixTimeStamp,
    ) -> Result<(), LiError> {
        let symbol_count = symbols.len();
        let (read_tx, mut read_rx) = mpsc::channel(100000);

        let share_cache = Arc::new(tokio::sync::Mutex::new(Vec::<R>::new()));
        for symbol in symbols {
            let tx_clone = read_tx.clone();
            let kline_fetcher = self.kline_fetcher_factory.create_fetcher();
            // NEXT： 这里1000变成参数化，现在是历史数据无所谓。但是实盘需要准确一点
            let interval = self.interval.clone();
            let param = P::initial(symbol.symbol.clone(), 1000, interval.clone());
            let task_name = self.task_name.clone();
            tokio::spawn({
                let tx_clone = tx_clone.clone();
                let param = param.clone();
                let kline_fetcher = kline_fetcher;
                async move {
                    Self::fetch_symbol_data::<_>(
                        kline_fetcher,
                        param,
                        start_timestamp,
                        end_timestamp,
                        tx_clone,
                        &task_name,
                        interval.clone(),
                    )
                    .await;
                }
            });
        }
        drop(read_tx);
        let write_cache = share_cache.clone();
        let data_writer_clone = self.data_writer.clone();
        // 后台定时任务：定期把共享缓存刷新到数据库
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                // 1) 在持锁时 clone 一份数据并清空缓存
                let cloned_cache: Vec<R> = {
                    let mut cache = write_cache.lock().await;
                    if cache.is_empty() {
                        continue;
                    }
                    let cloned = cache.clone();
                    cache.clear();
                    cloned
                };
                if let Err(e) = data_writer_clone.write_batch(cloned_cache) {
                    error!("Failed to write batch data: {}", e);
                }
            }
        });
        // 在主线程中接收结果并串行插入数据库
        for _ in 0..symbol_count {
            if let Some(result) = read_rx.recv().await {
                match result {
                    Ok((symbol, vo)) => {
                        let po_vec = vo.iter().map(|v| R::from_source(Some(&symbol), v)).collect::<Vec<_>>();
                        share_cache.lock().await.extend(po_vec);
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

    pub async fn fetch_symbol_data<T>(
        kline_fetcher: T,
        param: P,
        start_time: u64,
        end_time: u64,
        tx: mpsc::Sender<Result<(String, Vec<V>), YueError>>,
        task_name: &str,
        interval: HistoryInterval,
    ) where
        T: HistoryFetcher<P, V> + Send + Sync + 'static,
        R: HistoryPO<Source = V> + Clone,
    {
        debug!(
            "update {} -> symbol: {}, time from {} to {}",
            task_name,
            param.get_symbol(),
            unix_2_readable(&start_time),
            unix_2_readable(&end_time)
        );
        match kline_fetcher
            .get_all_kline_data(param.clone(), Some(interval), Some(start_time), Some(end_time), tx.clone())
            .await
        {
            Ok(len) => {
                debug!("{}:Fetched {} klines for symbol {}:", task_name, len, param.get_symbol(),);
            }
            Err(e) => {
                error!("{}:Error fetching klines for symbol {}: {}", task_name, param.get_symbol(), e);
            }
        };
    }
}

#[async_trait]
impl<F, P, R, V, D> AsyncRepeatTask for HistoryDataTask<F, P, R, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + Unpin + 'static,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + Clone + 'static,
    R: HistoryPO<Source = V> + Send + Sync + Clone + 'static,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync + Clone + 'static,
{
    ///
    /// 初始化任务特点
    /// 1. symbol为全集不为全部
    /// 2. 时间范围为设定的最早时间到现在
    ///
    async fn initial_data(&self) -> Result<(), LiError> {
        if !self.data_writer.is_empty().unwrap() {
            info!("{} database is not empty, skip initial history data fetch", self.task_name);
            return Ok(());
        }

        let earliest_time = self
            .exchange_dashboard
            .get_earliest_timestamp(Some(HistoryInterval::FiveMinutes))
            .ok_or_else(|| LiError::CustomError("Failed to get earliest timestamp from exchange_dashboard".to_string()))?;
        let symbols: Vec<TradingSymbol> = match self.symbol_type {
            SymbolType::Spot => self.exchange_dashboard.spot_all_symbols(),
            SymbolType::Swap => self.exchange_dashboard.swap_all_symbols(),
            _ => {
                return Err(LiError::CustomError(format!(
                    "Unsupported symbol type {:?} in InitialHistoryTask",
                    self.symbol_type
                )));
            }
        }
        .read()
        .unwrap()
        .clone()
        .into_iter()
        .filter(|symbol| symbol.quote_asset == "USDT")
        .collect();
        let symbol_count = symbols.len();
        let end_timestamp = self.interval.get_now_close_unix_ms_utc();
        info!(
            "inital data from {} to  {} fetch {},symbol num:{}",
            unix_2_readable(&earliest_time),
            unix_2_readable(&end_timestamp),
            self.task_name,
            symbol_count
        );

        self.fetch_and_write_history_data(symbols, earliest_time, end_timestamp).await
    }

    ///
    /// 初始化任务特点
    /// 1. symbol为正在交易的数据
    /// 2. 时间范围为过去的一个interval
    ///
    async fn execute(&self) -> Result<(), LiError> {
        let symbols: Vec<TradingSymbol> = match self.symbol_type {
            SymbolType::Spot => self.exchange_dashboard.spot_trading_symbols(),
            SymbolType::Swap => self.exchange_dashboard.swap_trading_symbols(),
            _ => {
                return Err(LiError::CustomError(format!(
                    "Unsupported symbol type {:?} in InitialHistoryTask",
                    self.symbol_type
                )));
            }
        };
        let symbol_count = symbols.len();

        let earliest_time = self.interval.get_now_close_unix_ms_utc() - self.interval.to_milliseconds();
        let end_timestamp = earliest_time + self.interval.to_milliseconds() - 1;

        info!(
            "refresh data from {} to {} fetch {},symbol num:{}",
            unix_2_readable(&earliest_time),
            unix_2_readable(&end_timestamp),
            self.task_name,
            symbol_count
        );

        let end_timestamp = self.interval.get_now_close_unix_ms_utc();
        self.fetch_and_write_history_data(symbols, earliest_time, end_timestamp).await
    }

    fn task_name(&self) -> &str {
        &self.task_name
    }
}

#[cfg(test)]
mod tests {
    use crate::binance::models::po::KlinePo;

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

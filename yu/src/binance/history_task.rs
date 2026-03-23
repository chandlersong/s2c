use crate::binance::bn_dashboard::TradingSymbol;
use crate::binance::models::po::DuckDBPO;
use crate::errors::YuError;
use crate::exchange::{ExchangeDashBoard, HistoryFetcherFactory};
use actix::Recipient;
use async_trait::async_trait;
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use li::tools::time::{unix_2_readable, UnixTimeStamp};
use log::{debug, error, info};
use std::sync::Arc;
use yue::binance::bn_models::common::{HistoryVo, SymbolType, ToRequestBuilder};
use yue::binance::history_data::{HistoryFetcher, MuteHistoryParam};
use yue::models::HistoryInterval;
use yue::query_message::{BatchInsert, Count, UNKNOWN_ROW};

pub trait HistoryDataWriter<O: DuckDBPO, D: ExchangeDashBoard<TradingSymbol = TradingSymbol>>: Send + Sync {
    ///
    /// 批量写入历史数据
    ///
    fn write_batch(&self, data: Vec<O>) -> Result<(), YuError>;

    ///
    /// 数据库为是否为空
    ///
    fn is_empty(&self) -> Result<bool, YuError>;
}

/// 初始化的历史数据任务，每次启动的时候，都会调用
/// NEXT：写一个实时更新的task
#[derive(Clone)]
pub struct HistoryDataTask<F, P, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V>,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync,
    V: HistoryVo + Clone + Sync + Send,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync + 'static,
{
    kline_fetcher_factory: F,
    exchange_dashboard: Arc<D>,
    batch_writer: Recipient<BatchInsert<V>>,
    db_empty_checker: Recipient<Count>,
    task_name: String,
    symbol_type: SymbolType,
    interval: HistoryInterval,
}

impl<F, P, V, D> HistoryDataTask<F, P, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + 'static,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + 'static,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync,
{
    pub fn new(
        factory: F,
        exchange_dashboard: Arc<D>,
        batch_writer: Recipient<BatchInsert<V>>,
        db_empty_checker: Recipient<Count>,
        task_name: String,
        symbol_type: SymbolType,
        interval: Option<HistoryInterval>,
    ) -> Self {
        let actual_interval = interval.unwrap_or_else(|| HistoryInterval::FiveMinutes);
        HistoryDataTask {
            kline_fetcher_factory: factory,
            exchange_dashboard,
            batch_writer,
            db_empty_checker,
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
        let mut handles = Vec::new();
        for symbol in symbols {
            let kline_fetcher = self.kline_fetcher_factory.create_fetcher();
            // NEXT： 这里1000变成参数化，现在是历史数据无所谓。但是实盘需要准确一点
            let interval = self.interval.clone();
            let param = P::initial(symbol.symbol.clone(), 1000, interval.clone());
            let task_name = self.task_name.clone();
            let reception = self.batch_writer.clone();
            let handle = tokio::spawn({
                let param = param.clone();
                let kline_fetcher = kline_fetcher;
                async move {
                    Self::fetch_symbol_data::<_>(
                        kline_fetcher,
                        param,
                        start_timestamp,
                        end_timestamp,
                        &task_name,
                        interval.clone(),
                        reception,
                    )
                    .await;
                }
            });
            handles.push(handle);
        }
        for h in handles {
            match h.await {
                Ok(_) => {}
                Err(join_err) => {
                    error!("{}", join_err);
                }
            }
        }
        // 在主线程中接收结果并串行插入数据
        info!("finish fetch {}", self.task_name);
        Ok(())
    }

    pub async fn fetch_symbol_data<T>(
        kline_fetcher: T,
        param: P,
        start_time: u64,
        end_time: u64,
        task_name: &str,
        interval: HistoryInterval,
        recipient: Recipient<BatchInsert<V>>,
    ) where
        T: HistoryFetcher<P, V> + Send + Sync + 'static,
    {
        debug!(
            "update {} -> symbol: {}, time from {} to {}",
            task_name,
            param.get_symbol(),
            unix_2_readable(&start_time),
            unix_2_readable(&end_time)
        );
        match kline_fetcher
            .get_all_kline_data(param.clone(), Some(interval), Some(start_time), Some(end_time), recipient, true)
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
impl<F, P, V, D> AsyncRepeatTask for HistoryDataTask<F, P, V, D>
where
    F: HistoryFetcherFactory<Param = P, Output = V> + Clone + Send + Sync + Unpin + 'static,
    P: MuteHistoryParam + ToRequestBuilder + Clone + Send + Sync + 'static,
    V: HistoryVo + Clone + Send + Sync + Clone + 'static,
    D: ExchangeDashBoard<TradingSymbol = TradingSymbol> + Send + Sync + Clone + 'static,
{
    ///
    /// 初始化任务特点
    /// 1. symbol为全集不为全部
    /// 2. 时间范围为设定的最早时间到现在
    ///
    async fn initial_data(&self) -> Result<(), LiError> {
        let count = self
            .db_empty_checker
            .send(Count::new())
            .await
            .map_err(|e| LiError::CustomError(format!("Failed to send Count message to db_empty_checker in {}: {}", self.task_name, e)))?;
        if count == 0 {
            info!("{} database is not empty, skip initial history data fetch", self.task_name);
            return Ok(());
        } else if count == UNKNOWN_ROW {
            return Err(LiError::CustomError(format!("DB connection fail {}", self.task_name)));
        };

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

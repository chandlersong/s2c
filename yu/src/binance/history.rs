use crate::binance::bn_backend_service::get_swap_funding_rate_table;
use crate::binance::bn_dashboard::{BinanceDashboardWatcher, InstrumentProvider};
use crate::binance::models::po::{FundingRatePo, KlinePo};
use crate::config::AppConfig;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use async_trait::async_trait;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{debug, error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandlerTrait, HistoryFetcherImpl, HistoryFetcherTrait, ShareHistoryBatchHandler};
use yue::errors::YueError;
use yue::models::{HistoryInterval, InstrumentType};
use yue::query_message::{BatchInsertPayload, QueryCommand};

pub(crate) const INITIAL_HISTORY_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub(crate) struct HistoryKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
    instrument_provider: InstrumentProvider,
}

impl HistoryKlineSaver {
    pub fn new(db: DuckTableTableChannel<KlinePo>, instrument_provider: InstrumentProvider) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { db, instrument_provider }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for HistoryKlineSaver {
    ///
    /// 从provider里面，获取数据库里面的id。如果不存在，则跳过那一条，然后保存
    ///
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), YueError> {
        let po_vec = batch_data
            .into_iter()
            .filter_map(|kline| {
                let Some(symbol) = kline.symbol.as_deref() else {
                    debug!("SpotHistoryKline skipped because Binance kline has no symbol");
                    return None;
                };
                let Some(instrument) = self.instrument_provider.get_instrument(symbol) else {
                    debug!("SpotHistoryKline skipped because instrument {} was not found", symbol);
                    return None;
                };
                Some(KlinePo::from_binance_kline(kline, instrument.id as u64))
            })
            .collect::<Vec<_>>();
        if po_vec.is_empty() {
            return Ok(());
        }
        let batch_command = QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(po_vec));
        if let Err(e) = self.db.send(batch_command).await {
            error!("SpotHistoryKline send error {:?}", e);
        };

        Ok(())
    }
}

struct FundingRateSaver {
    _db: DuckTableTableChannel<FundingRatePo>,
}

impl FundingRateSaver {
    pub fn new(db: DuckTableTableChannel<FundingRatePo>) -> ShareHistoryBatchHandler<FundingRate> {
        Arc::new(Self { _db: db }) as Arc<dyn HistoryBatchHandlerTrait<FundingRate> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<FundingRate> for FundingRateSaver {
    async fn handle(&self, _batch_data: Vec<FundingRate>) -> Result<(), YueError> {
        // TODO: 以后改吧
        // let po_vec = batch_data.iter().map(|v| FundingRatePo::from(v.clone())).collect::<Vec<FundingRatePo>>();
        //
        // let batch_command = QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(po_vec));
        // if let Err(e) = self.db.send(batch_command).await {
        //     error!("SpotHistoryKline send error {:?}", e);
        // }
        // Ok(())
        todo!("暂时不保存funding rate数据，后续可以考虑保存funding rate数据");
    }
}

///
///  本模块主要是为了把历史的数据的取得工作
///
///  FUTURE：
/// 1. 现在K线等都是通过restful来获得，但是在仓库以文件方式保存。可以通过那样来初始化。
///

///
/// 初始化K线数据
/// 1. 确定开始的时间和结束时间
///     - 开始时间: now之前的保存时间一个的时间周期。
///     - 结束时间，上个周期的-1ms
///
pub async fn initial_kline(
    instrument_provider: InstrumentProvider,
    data_retention_ms: u64,
    interval: HistoryInterval,
    db: DuckTableTableChannel<KlinePo>,
    end_ms: Option<u64>,
) -> Result<(), YuError> {
    let symbol_type = instrument_provider.instrument_type();
    let (tx, rx) = oneshot::channel();
    if let Err(e) = db.send(QueryCommand::GetCount(tx)).await {
        error!("binance {} HistoryKline query count error {:?}", symbol_type, e);
    }

    match rx.await {
        Ok(Ok(count)) => {
            if count > 0 {
                info!("binance {} kline table非空，现存{}，跳过初始化", symbol_type, count);
                return Ok(());
            }
        }
        _ => {
            let error_msg = format!("binance {} HistoryKline query count error", symbol_type);
            return Err(YuError::new(error_msg.as_str()));
        }
    }

    let now = end_ms.unwrap_or_else(unix_time_now_u64_utc);
    let instruments = instrument_provider.instruments(None);
    //kline会有一个会取到未闭合K线的问题。所以我这里也就去取上一根K线的之前的一毫秒。
    //例如现在是36分，通过restful能够取到35～39的K线，但是未闭合。所以我直接取34.59.59.999这个时间点的K线。所以就规避了。
    let start = interval.get_close_unix_ms(now - data_retention_ms);
    let end = interval.get_close_unix_ms(now).saturating_sub(1);
    info!(
        "开始 {} kline 初始化数据，from {} to {}",
        symbol_type,
        unix_2_readable(&start),
        unix_2_readable(&end)
    );
    let saver = HistoryKlineSaver::new(db, instrument_provider);
    let total = instruments.len();
    let mut last_progress_log = tokio::time::Instant::now();
    let mut completed = 0;
    for instrument in instruments {
        let fetcher = match &symbol_type {
            InstrumentType::Spot => HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND),
            InstrumentType::Swap => HistoryFetcherImpl::kline(&SWAP_KLINE_HISTORY_COMMAND),
            _ => {
                error!("binance {} kline 初始化不支持的交易类型", symbol_type);
                completed += 1;
                continue;
            }
        };
        let request = CommonRequestBuilder::new(instrument.symbol, 1000, interval.clone());
        if let Err(error) = fetcher
            .get_all_kline_data(request, Some(interval.clone()), Some(start), Some(end), Some(saver.clone()), true)
            .await
        {
            error!("{}", error);
        }
        completed += 1;

        if last_progress_log.elapsed() >= INITIAL_HISTORY_PROGRESS_LOG_INTERVAL {
            info!("binance {} kline 初始化进度：{}/{} 个交易对已完成", symbol_type, completed, total);
            last_progress_log = tokio::time::Instant::now();
        }
    }
    info!("binance {} kilin初始化完成", symbol_type);

    Ok(())
}

///
/// 开始获取funding rate
/// 1. 初始化funding rate
/// 2.
///
pub async fn start_sync_funding_rate(
    symbols: Vec<String>,
    config: &AppConfig,
    interval: HistoryInterval,
    dash_board_watch: BinanceDashboardWatcher,
) -> Result<(), YuError> {
    let now = unix_time_now_u64_utc();
    let start = interval.get_close_unix_ms(now - config.get_data_retention_ms());
    let end = interval.get_close_unix_ms(now).saturating_sub(1);
    info!("开始资金费率初始化数据，from {} to {}", unix_2_readable(&start), unix_2_readable(&end));
    let (tx, rx) = oneshot::channel();
    let db = get_swap_funding_rate_table();
    if let Err(e) = db.send(QueryCommand::GetCount(tx)).await {
        error!("binance funding rate query count error {:?}", e);
    }

    match rx.await {
        Ok(Ok(count)) => {
            if count > 0 {
                info!("binance funding rate table非空，现存{}，跳过初始化", count);
                return Ok(());
            } else {
                info!("binance funding rate table为空空，开始初始化");
                sync_funding_rate(symbols.clone(), start, end, interval.clone()).await?;
            }
        }
        _ => {
            return Err(YuError::new("FundingRateHistoryKline query count error"));
        }
    }

    let mut rx = dash_board_watch.subscribe();

    tokio::spawn(async move {
        loop {
            let interval_clone = interval.clone();
            let last_update = end.saturating_add(1);
            //感觉有点小问题。如果长时间不更新，就不更新了。有点麻烦
            match rx.changed().await {
                Ok(_) => {
                    let snapshot = (*rx.borrow()).clone();
                    info!("重新开始更新binance funding rate 数目是:{}", snapshot.live_swap.len());
                    let trading_swaps_symbols = snapshot.live_swap.clone().into_iter().map(|(_, inst)| inst.symbol).collect();
                    let start = interval_clone.get_close_unix_ms(last_update);
                    let last_update = interval_clone.get_now_close_unix_ms_utc();
                    let end = last_update.saturating_sub(1);
                    if let Err(e) = sync_funding_rate(trading_swaps_symbols, start, end, interval_clone).await {
                        error!("同步sync funding rate错误，跳过本次：{}", e);
                    }
                }
                Err(e) => {
                    error!("同步sync funding rate订阅错误，跳过本次:{}", e);
                }
            }
        }
    });

    Ok(())
}

pub async fn sync_funding_rate(symbols: Vec<String>, start: u64, end: u64, interval: HistoryInterval) -> Result<(), YuError> {
    let db = get_swap_funding_rate_table();
    let saver = FundingRateSaver::new(db);
    for symbol in symbols {
        let fetcher = HistoryFetcherImpl::swap_funding_rate();
        let request = CommonRequestBuilder::new(symbol, 1000, interval.clone());
        if let Err(error) = fetcher
            .get_all_kline_data(request, Some(interval.clone()), Some(start), Some(end), Some(saver.clone()), true)
            .await
        {
            error!("{}", error);
        }
    }

    Ok(())
}

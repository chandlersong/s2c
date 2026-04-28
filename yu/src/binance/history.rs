use crate::binance::bn_backend_service::get_swap_funding_rate_table;
use crate::binance::bn_dashboard::{BinanceDashboard, BinanceDashboardWatcher};
use crate::binance::bn_duck_db::DuckTableTableChannel;
use crate::binance::models::po::{FundingRatePo, KlinePo};
use crate::config::AppConfig;
use crate::errors::YuError;
use async_trait::async_trait;
use governor::Jitter;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc};
use log::{error, info};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::error::RecvError;
use tokio::sync::{oneshot, OwnedSemaphorePermit, Semaphore};
use tokio::time::timeout;
use yue::binance::bn_models::common::SymbolType;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandlerTrait, HistoryFetcherImpl, HistoryFetcherTrait, ShareHistoryBatchHandler};
use yue::errors::YueError;
use yue::models::HistoryInterval;
use yue::query_message::{BatchInsertPayload, QueryCommand};

pub(crate) struct HistoryKlineSaver {
    db: DuckTableTableChannel<KlinePo>,
}

impl HistoryKlineSaver {
    pub fn new(db: DuckTableTableChannel<KlinePo>) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { db }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for HistoryKlineSaver {
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), YueError> {
        let po_vec = batch_data.iter().map(|v| KlinePo::from(v.clone())).collect::<Vec<KlinePo>>();

        let batch_command = QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(po_vec));
        if let Err(e) = self.db.send(batch_command).await {
            error!("SpotHistoryKline send error {:?}", e);
        }
        Ok(())
    }
}

struct FundingRateSaver {
    db: DuckTableTableChannel<FundingRatePo>,
}

impl FundingRateSaver {
    pub fn new(db: DuckTableTableChannel<FundingRatePo>) -> ShareHistoryBatchHandler<FundingRate> {
        Arc::new(Self { db }) as Arc<dyn HistoryBatchHandlerTrait<FundingRate> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<FundingRate> for FundingRateSaver {
    async fn handle(&self, batch_data: Vec<FundingRate>) -> Result<(), YueError> {
        let po_vec = batch_data.iter().map(|v| FundingRatePo::from(v.clone())).collect::<Vec<FundingRatePo>>();

        let batch_command = QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(po_vec));
        if let Err(e) = self.db.send(batch_command).await {
            error!("SpotHistoryKline send error {:?}", e);
        }
        Ok(())
    }
}

async fn acquire_permit_with_retry(sem: Arc<Semaphore>) -> Result<OwnedSemaphorePermit, YuError> {
    loop {
        // 使用 timeout 包裹 acquire_owned，以免无限等待
        match timeout(Duration::from_secs(10), sem.clone().acquire_owned()).await {
            Ok(acquire_res) => {
                return match acquire_res {
                    Ok(permit) => {
                        // 成功获取 permit
                        Ok(permit)
                    }
                    Err(acq_err) => {
                        // AcquireError：semaphore 已关闭（通常是优雅关机），不可重试
                        // 把它当成致命错误上抛
                        Err(YuError::new(&format!("semaphore closed or acquire failed: {}", acq_err)))
                    }
                };
            }
            Err(_elapsed) => {
                //因为初始化一般都会比较长，所以也就睡的长一点
                let jitter = Jitter::up_to(Duration::from_secs(20));
                let sleep_seconds = jitter + Duration::from_secs(10);
                tokio::time::sleep(sleep_seconds).await;
                continue;
            }
        }
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
    symbol_type: SymbolType,
    spot_symbols: Vec<String>,
    config: &AppConfig,
    interval: HistoryInterval,
    db: DuckTableTableChannel<KlinePo>,
) -> Result<(), YuError> {
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

    let now = unix_time_now_u64_utc();
    //kline会有一个会取到未闭合K线的问题。所以我这里也就去取上一根K线的之前的一毫秒。
    //例如现在是36分，通过restful能够取到35～39的K线，但是未闭合。所以我直接取34.59.59.999这个时间点的K线。所以就规避了。
    let start = interval.get_close_unix_ms(now - config.get_data_retention_ms());
    let end = interval.get_close_unix_ms(now).saturating_sub(1);
    info!(
        "开始 {} kline 初始化数据，from {} to {}",
        symbol_type,
        unix_2_readable(&start),
        unix_2_readable(&end)
    );
    // 使用并发任务来处理多个交易对的历史数据下载，但使用 Semaphore 限制最大并发数，
    // 等待所有任务完成后再返回。这样在低配机器上也能控制资源使用。
    let sem = Arc::new(Semaphore::new(10));
    let saver = HistoryKlineSaver::new(db);
    let mut handles = Vec::with_capacity(spot_symbols.len());
    for spot_symbol in spot_symbols {
        let sem = sem.clone();
        let symbol = spot_symbol.clone();
        // 拷贝需要的值到任务里（start/end 是 Copy）
        let start_ts = start;
        let end_ts = end;
        let saver_clone = saver.clone();
        let handle = tokio::spawn(async move {
            // 获取一个 OwnedPermit，保证在任务完成前不会释放
            if let Err(e) = acquire_permit_with_retry(sem).await {
                error!("初始化spot kline的获取锁出错: {}", e);
            }

            let spot_kline_fetch = match symbol_type {
                SymbolType::Spot => HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND),
                SymbolType::Swap => HistoryFetcherImpl::kline(&SWAP_KLINE_HISTORY_COMMAND),
                _ => return Err(YueError::new("类型不支持")),
            };

            let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, HistoryInterval::FiveMinutes);

            spot_kline_fetch
                .get_all_kline_data(
                    base_param,
                    Some(HistoryInterval::FiveMinutes),
                    Some(start_ts),
                    Some(end_ts),
                    Some(saver_clone),
                    true,
                )
                .await
        });

        handles.push(handle);
    }

    // 等待所有任务完成，若有任务返回错误则提前返回错误
    for h in handles {
        match h.await {
            Ok(Ok(_)) => {}
            // 将内部的 yue::errors::YueError 转换为 crate::errors::YuError（实现了 From）
            Ok(Err(e)) => {
                error!("{}", e);
            }
            // join error => 转为 YuError
            Err(join_err) => {
                error!("{}", join_err);
            }
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
    sync_funding_rate(symbols.clone(), start, end, interval.clone()).await?;
    let mut rx = dash_board_watch.subscribe();

    tokio::spawn(async move {
        loop {
            let interval_clone = interval.clone();
            let last_update = end.saturating_add(1);
            //感觉有点小问题。如果长时间不更新，就不更新了。有点麻烦
            match rx.changed().await {
                Ok(_) => {
                    let snapshot = (*rx.borrow()).clone();
                    info!("重新开始更新binance funding rate 数目是:{}", snapshot.swap_trading_symbols.len());
                    let trading_swaps_symbols = snapshot.swap_trading_symbols.clone().into_iter().map(|s| s.symbol).collect();
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
            }
        }
        _ => {
            return Err(YuError::new("FundingRateHistoryKline query count error"));
        }
    }
    //kline会有一个会取到未闭合K线的问题。所以我这里也就去取上一根K线的之前的一毫秒。
    //例如现在是36分，通过restful能够取到35～39的K线，但是未闭合。所以我直接取34.59.59.999这个时间点的K线。所以就规避了。

    // 使用并发任务来处理多个交易对的历史数据下载，但使用 Semaphore 限制最大并发数，
    // 等待所有任务完成后再返回。这样在低配机器上也能控制资源使用。
    let sem = Arc::new(Semaphore::new(5));
    let saver = FundingRateSaver::new(db);
    let mut handles = Vec::with_capacity(symbols.len());
    for spot_symbol in symbols {
        let sem = sem.clone();
        let symbol = spot_symbol.clone();
        // 拷贝需要的值到任务里（start/end 是 Copy）
        let start_ts = start;
        let end_ts = end;
        let saver_clone = saver.clone();
        let interval_clone = interval.clone();
        let handle = tokio::spawn(async move {
            // 获取一个 OwnedPermit，保证在任务完成前不会释放
            if let Err(e) = acquire_permit_with_retry(sem).await {
                error!("初始化funding rate 的获取锁出错: {}", e);
            }

            let spot_kline_fetch = HistoryFetcherImpl::swap_funding_rate();
            let base_param = CommonRequestBuilder::new(symbol.to_string(), 1000, interval_clone.clone());

            spot_kline_fetch
                .get_all_kline_data(base_param, Some(interval_clone), Some(start_ts), Some(end_ts), Some(saver_clone), true)
                .await
        });

        handles.push(handle);
    }

    // 等待所有任务完成，若有任务返回错误则提前返回错误
    for h in handles {
        match h.await {
            Ok(Ok(_)) => {}
            // 将内部的 yue::errors::YueError 转换为 crate::errors::YuError（实现了 From）
            Ok(Err(e)) => {
                error!("{}", e);
            }
            // join error => 转为 YuError
            Err(join_err) => {
                error!("{}", join_err);
            }
        }
    }

    Ok(())
}

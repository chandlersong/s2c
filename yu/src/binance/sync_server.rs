use crate::binance::bn_backend_service::{get_spot_kline_table, get_spot_order_book_service, get_swap_kline_table};
use crate::binance::bn_dashboard::{BinanceDashboard, InstrumentProvider, SpotInstrumentProvider, SwapInstrumentProvider};
use crate::binance::db_consts::BinanceTables;
use crate::binance::duckdb_repository::{
    BNInstrumentRepository, BinanceFundingRateRepository, BinanceKlineRepository, BinanceOptionInstrumentRepository, BinanceOptionKlineRepository,
    BinanceOptionMarketPriceRepository, get_binance_option_instrument_repo, get_default_binance_option_kline_repo,
    get_default_binance_option_market_price_repo, get_default_funding_rate_repo, get_default_spot_kline_repo, get_default_swap_kline_repo,
    get_instrument_repo_for_production,
};
use crate::binance::history::{INITIAL_HISTORY_PROGRESS_LOG_INTERVAL, initial_kline};
use crate::binance::models::po::{
    BinanceInstrument, BinanceOptionInstrumentPo, BinanceOptionKlinePo, BinanceOptionMarketPricePo, FundingRatePo, KlinePo,
};
use crate::config::AppConfig;
use crate::data_integrity::check::{BinarySearchDS, DuckDBBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, ServerMessage};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tonic::Status;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::option_api::{BinanceOptionExchangeApi, default_binance_option_api};
use yue::binance::order_book::OrderBookService;
use yue::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandlerTrait, HistoryFetcherImpl, HistoryFetcherTrait, ShareHistoryBatchHandler};
use yue::errors::YueError as ExchangeError;
use yue::models::{HistoryInterval, InstrumentType};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::models::SpotStreamTradeRecordPo;
    use crate::sync::models::grpc_sync::{HistoryType, InstrumentType, history_entry};

    // 测试目的：验证 Spot Trade PO 映射为 Binance Trade 历史条目时保留必要字段。
    // 准备数据/过程：构造包含可选成交时间和买方做市标记的 PO，并转换为 HistoryEntry。
    // 预期结果：历史类型、交易所类型、instrument ID 及 BinanceTrade payload 字段正确。
    #[test]
    fn spot_trade_po_converts_to_binance_trade_history_entry() {
        let trade = SpotStreamTradeRecordPo {
            id: 3,
            event_time: 1_700_000_000_001,
            inst_id: 42,
            trade_id: 17,
            price: 100.5,
            qty: 2.0,
            trade_time: Some(1_700_000_000_000),
            is_buyer_maker: Some(true),
            created_at: 0,
        };

        let entry = trade.to_spot_history_entry();

        assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceTrade);
        assert_eq!(InstrumentType::try_from(entry.instrument_type).unwrap(), InstrumentType::Binance);
        assert_eq!(entry.inst_id, 42);
        let Some(history_entry::Payload::BinanceTrade(payload)) = entry.payload else {
            panic!("expected Binance Trade payload");
        };
        assert_eq!(payload.id, 3);
        assert_eq!(payload.event_time, trade.event_time);
        assert_eq!(payload.trade_id, 17);
        assert_eq!(payload.trade_time, Some(1_700_000_000_000));
        assert_eq!(payload.is_buyer_maker, Some(true));
    }

    // 测试目的：验证 Binance Spot Kline PO 会映射为正确的同步历史条目。
    // 准备数据/过程：构造完整的 KlinePo，使用 instrument id 42 转换为 HistoryEntry。
    // 预期结果：历史类型、交易所类型、instrument id 和 Kline payload 字段保持正确。
    #[test]
    fn kline_po_converts_to_binance_spot_history_entry() {
        let kline = KlinePo {
            id: 3,
            inst_id: 42,
            candle_begin_time: 1_700_000_000_000,
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 2.0,
            quote_volume: 210.0,
            number_of_trades: 20,
            taker_buy_base_asset_volume: 1.0,
            taker_buy_quote_asset_volume: 105.0,
            close_time: 1_700_000_299_999,
            interval: 1,
            first_trade_id: Some(1),
            last_trade_id: Some(20),
        };

        let entry = kline.to_spot_history_entry();

        assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceSpot);
        assert_eq!(InstrumentType::try_from(entry.instrument_type).unwrap(), InstrumentType::Binance);
        assert_eq!(entry.inst_id, 42);
        let Some(history_entry::Payload::BinanceKline(payload)) = entry.payload else {
            panic!("expected Binance Kline payload");
        };
        assert_eq!(payload.id, 3);
        assert_eq!(payload.candle_begin_time, kline.candle_begin_time);
        assert_eq!(payload.close, kline.close);
        assert_eq!(payload.first_trade_id, Some(1));
    }

    // 测试目的：验证 FundingRatePo 转换为 Binance Swap 历史条目时保留资金费率字段和 nullable mark_price。
    // 准备数据/过程：构造 mark_price 为空的 FundingRatePo，并转换成 HistoryEntry。
    // 预期结果：同步类型为 BinanceSwapFundingRate，instrument ID、毫秒时间和空 mark_price 均保持不变。
    #[test]
    fn funding_rate_po_converts_to_binance_swap_history_entry() {
        let funding_rate = FundingRatePo {
            id: 3,
            inst_id: 42,
            funding_rate: 0.0001,
            funding_time: 1_700_000_000_123,
            mark_price: None,
        };

        let entry = funding_rate.to_swap_history_entry();

        assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceSwapFundingRate);
        assert_eq!(InstrumentType::try_from(entry.instrument_type).unwrap(), InstrumentType::Binance);
        assert_eq!(entry.inst_id, 42);
        let Some(history_entry::Payload::BinanceFundingRate(payload)) = entry.payload else {
            panic!("expected Binance Funding Rate payload");
        };
        assert_eq!(payload.id, 3);
        assert_eq!(payload.funding_time, 1_700_000_000_123);
        assert_eq!(payload.funding_rate, 0.0001);
        assert_eq!(payload.mark_price, None);
    }
}

pub(crate) const CHECK_HISTORY_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
pub struct SubscribeSpotOrderBookResult {
    pub level: u16,
    pub symbols: HashMap<String, BinanceInstrument>,
    pub order_book_service: Arc<OrderBookService>,
}

pub async fn subscribe_spot_order_book(app_config: &AppConfig) -> Result<SubscribeSpotOrderBookResult, YuError> {
    let order_book_service = get_spot_order_book_service().await;
    let instrument_repo = get_instrument_repo_for_production(None);
    let ws_config = match &app_config.binance {
        Some(ws) => ws,
        None => {
            info!("binance_websocket 配置未启用，跳过 WebSocket 任务");
            return Err(YuError::ConfigError("binance_websocket 配置未启用".to_string()));
        }
    };

    let spot_config = match &ws_config.spot_stream {
        Some(spot) => spot,
        None => {
            info!("binance_websocket.spot 配置未启用，跳过 Spot WebSocket 任务");
            return Err(YuError::ConfigError("binance_websocket.spot 配置未启用".to_string()));
        }
    };
    let level: u16;
    let mut symbols: HashMap<String, BinanceInstrument> = HashMap::new();
    // 根据配置订阅深度流
    if let Some(depth_config) = &spot_config.depth {
        if depth_config.enabled() && !depth_config.symbols.is_empty() {
            let update_speed = &depth_config.update_speed();
            for symbol in depth_config.symbols.iter() {
                if let Ok(Some(inst)) = instrument_repo.get_by_symbol(symbol, InstrumentType::Spot).await {
                    symbols.insert(symbol.clone(), inst);
                } else {
                    error!("在订阅{}的订单簿时，未找到对应的Spot instrument，跳过订阅", symbol);
                    continue;
                }
                if let Err(e) = order_book_service.subscribe_order_book(symbol, update_speed) {
                    error!("在订阅{},speed:{}的订单簿时出错{}", symbol, update_speed, e);
                    continue;
                }
                info!("订阅{}:{}的订单簿", symbol, update_speed);
            }
            level = depth_config.levels() as u16;
        } else {
            info!("binance_websocket.spot.depth 未启用或没有配置symbols，跳过深度流订阅");
            return Err(YuError::ConfigError(
                "binance_websocket.spot.depth 未启用或没有配置symbols，跳过深度流订阅".to_string(),
            ));
        }
    } else {
        info!("binance_websocket.spot.depth 配置未启用，跳过深度流订阅");
        return Err(YuError::ConfigError(
            "binance_websocket.spot.depth 配置未启用，跳过深度流订阅".to_string(),
        ));
    }

    if symbols.is_empty() {
        return Err(YuError::ConfigError(
            "binance_websocket.spot.depth 配置未启用或没有配置symbols，跳过深度流订阅".to_string(),
        ));
    }

    Ok(SubscribeSpotOrderBookResult {
        order_book_service,
        level,
        symbols,
    })
}

struct BinanceGapKlineSaver {
    repo: BinanceKlineRepository,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
}

impl BinanceGapKlineSaver {
    fn new(repo: BinanceKlineRepository, history_tx: mpsc::Sender<HistoryEntry>, inst_id: u64) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { repo, history_tx, inst_id }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for BinanceGapKlineSaver {
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), ExchangeError> {
        let klines = batch_data
            .into_iter()
            .map(|kline| KlinePo::from_binance_kline(kline, self.inst_id))
            .collect::<Vec<_>>();
        for kline in klines {
            if let Err(error) = self.repo.insert_kline(kline.clone()).await {
                error!("Error inserting Binance Spot Kline into repository: {}", error);
            }
            if let Err(error) = self.history_tx.send(kline.to_spot_history_entry()).await {
                error!("Error sending Binance Spot Kline history entry: {}", error);
            }
        }
        Ok(())
    }
}

pub struct BinanceSpotSyncHistoryService {
    data_retention_ms: u64,
    inst_provider: InstrumentProvider,
    kline_repo: BinanceKlineRepository,
    binary_search_ds: BinarySearchDS,
    history_tx: mpsc::Sender<HistoryEntry>,
    interval: HistoryInterval,
}

impl BinanceSpotSyncHistoryService {
    pub fn for_production(
        data_retention_ms: u64,
        history_tx: mpsc::Sender<HistoryEntry>,
        dashboard: Arc<BinanceDashboard>,
        interval: HistoryInterval,
    ) -> Arc<Self> {
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::SpotKline.table_name(),
            "candle_begin_time".to_string(),
            "inst_id".to_string(),
        );
        Arc::new(Self {
            data_retention_ms,
            inst_provider: SpotInstrumentProvider::new_provider(dashboard),
            kline_repo: get_default_spot_kline_repo(None, None),
            binary_search_ds,
            history_tx,
            interval,
        })
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceSpotSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let klines = self.kline_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if klines.is_empty() {
            let message = ServerMessage {
                history: vec![],
                server_timestamp: unix_time_now_u64_utc(),
            };
            let _ = tx.send(Ok(message)).await;
            return Ok(());
        }

        for batch in klines.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(|kline| kline.to_spot_history_entry()).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self, end_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        let interval = self.interval.clone();
        let now_ms = end_ms.unwrap_or_else(|| interval.get_now_close_unix_ms_utc());
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);
        let instruments = self.inst_provider.instruments(None);
        let total = instruments.len();
        let mut completed = 0;
        let mut no_gap_count = 0;
        let mut last_progress_log = tokio::time::Instant::now();

        for instrument in instruments {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms < now_ms {
                let gaps_res = binary_search_gap(
                    instrument.id.to_string().as_str(),
                    "SPOT",
                    start_ms,
                    now_ms,
                    &interval,
                    self.binary_search_ds.clone(),
                    0,
                )?;
                let gaps = gaps_res.gaps;
                debug!("{} has {} gaps", instrument.symbol, gaps.len());
                no_gap_count = match gaps_res.level {
                    0 => no_gap_count + 1,
                    _ => no_gap_count,
                };
                for gap in gaps {
                    match gap {
                        ValidationGap::MissingData { start_time, end_time, .. } => {
                            let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
                            let request = CommonRequestBuilder::new(instrument.symbol.clone(), 1000, interval.clone());
                            let saver = BinanceGapKlineSaver::new(self.kline_repo.clone(), self.history_tx.clone(), instrument.id as u64);
                            fetcher
                                .get_all_kline_data(request, Some(interval.clone()), Some(start_time), Some(end_time), Some(saver), true)
                                .await?;
                        }
                        other => debug!("skip unsupported Binance history gap: {:?}", other),
                    }
                }
            }

            completed += 1;
            if last_progress_log.elapsed() >= CHECK_HISTORY_PROGRESS_LOG_INTERVAL {
                info!(
                    "binance Spot Kline history check progress: {}/{} instruments checked, {} instruments with gaps",
                    completed,
                    total,
                    completed - no_gap_count
                );
                last_progress_log = tokio::time::Instant::now();
            }
        }
        info!(
            "binance Spot Kline history check done: {}/{} instruments checked，{} instruments with gaps",
            completed,
            total,
            completed - no_gap_count
        );
        Ok(())
    }

    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let before = end_ms.unwrap_or_else(|| unix_time_now_u64_utc());
        let latest_timestamps = self.kline_repo.inst_id_with_lastest_timestamp_before(before).await?;
        initial_kline(
            self.inst_provider.clone(),
            self.data_retention_ms,
            self.interval.clone(),
            get_spot_kline_table(),
            end_ms,
            latest_timestamps,
        )
        .await
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        debug!("Binance Spot latest kline sync is driven by WebSocket");
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

struct BinanceSwapGapKlineSaver {
    repo: BinanceKlineRepository,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
}

impl BinanceSwapGapKlineSaver {
    fn new(repo: BinanceKlineRepository, history_tx: mpsc::Sender<HistoryEntry>, inst_id: u64) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { repo, history_tx, inst_id }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for BinanceSwapGapKlineSaver {
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), ExchangeError> {
        let klines = batch_data
            .into_iter()
            .map(|kline| KlinePo::from_binance_kline(kline, self.inst_id))
            .collect::<Vec<_>>();
        for kline in klines {
            if let Err(error) = self.repo.insert_kline(kline.clone()).await {
                error!("Error inserting Binance Swap Kline into repository: {}", error);
            }
            if let Err(error) = self.history_tx.send(kline.to_swap_history_entry()).await {
                error!("Error sending Binance Swap Kline history entry: {}", error);
            }
        }
        Ok(())
    }
}

pub struct BinanceSwapSyncHistoryService {
    data_retention_ms: u64,
    inst_provider: InstrumentProvider,
    kline_repo: BinanceKlineRepository,
    binary_search_ds: BinarySearchDS,
    history_tx: mpsc::Sender<HistoryEntry>,
    interval: HistoryInterval,
}

impl BinanceSwapSyncHistoryService {
    pub fn for_production(
        data_retention_ms: u64,
        history_tx: mpsc::Sender<HistoryEntry>,
        dashboard: Arc<BinanceDashboard>,
        interval: HistoryInterval,
    ) -> Arc<Self> {
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::SwapKline.table_name(),
            "candle_begin_time".to_string(),
            "inst_id".to_string(),
        );
        Arc::new(Self {
            data_retention_ms,
            inst_provider: SwapInstrumentProvider::new_provider(dashboard),
            kline_repo: get_default_swap_kline_repo(None, None),
            binary_search_ds,
            history_tx,
            interval,
        })
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceSwapSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let klines = self.kline_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if klines.is_empty() {
            let message = ServerMessage {
                history: vec![],
                server_timestamp: unix_time_now_u64_utc(),
            };
            let _ = tx.send(Ok(message)).await;
            return Ok(());
        }

        for batch in klines.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(|kline| kline.to_swap_history_entry()).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self, end_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        let interval = self.interval.clone();
        let now_ms = end_ms.unwrap_or_else(|| interval.get_now_close_unix_ms_utc());
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);
        let instruments = self.inst_provider.instruments(None);
        let total = instruments.len();
        let mut completed = 0;
        let mut no_gap_count = 0;
        let mut last_progress_log = tokio::time::Instant::now();

        for instrument in instruments {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms < now_ms {
                let gaps_res = binary_search_gap(
                    instrument.id.to_string().as_str(),
                    "SWAP",
                    start_ms,
                    now_ms,
                    &interval,
                    self.binary_search_ds.clone(),
                    0,
                )?;
                let gaps = gaps_res.gaps;
                debug!("{} has {} gaps", instrument.symbol, gaps.len());
                no_gap_count = match gaps_res.level {
                    0 => no_gap_count + 1,
                    _ => no_gap_count,
                };

                for gap in gaps {
                    match gap {
                        ValidationGap::MissingData { start_time, end_time, .. } => {
                            let fetcher = HistoryFetcherImpl::kline(&SWAP_KLINE_HISTORY_COMMAND);
                            let request = CommonRequestBuilder::new(instrument.symbol.clone(), 1000, interval.clone());
                            let saver = BinanceSwapGapKlineSaver::new(self.kline_repo.clone(), self.history_tx.clone(), instrument.id as u64);
                            fetcher
                                .get_all_kline_data(request, Some(interval.clone()), Some(start_time), Some(end_time), Some(saver), true)
                                .await?;
                        }
                        other => debug!("skip unsupported Binance Swap history gap: {:?}", other),
                    }
                }
            }

            completed += 1;
            if last_progress_log.elapsed() >= CHECK_HISTORY_PROGRESS_LOG_INTERVAL {
                info!(
                    "binance Swap Kline history check progress: {}/{} instruments checked,{} instruments with gaps",
                    completed,
                    total,
                    completed - no_gap_count
                );
                last_progress_log = tokio::time::Instant::now();
            }
        }
        info!(
            "binance Swap Kline history check done: {}/{} instruments checked, {} instruments with gaps",
            completed,
            total,
            completed - no_gap_count
        );
        Ok(())
    }

    async fn initial_history_data(&self, initial_before_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        let before = initial_before_ms.unwrap_or_else(|| unix_time_now_u64_utc());
        let latest_timestamps = self.kline_repo.inst_id_with_lastest_timestamp_before(before).await?;
        initial_kline(
            self.inst_provider.clone(),
            self.data_retention_ms,
            self.interval.clone(),
            get_swap_kline_table(),
            initial_before_ms,
            latest_timestamps,
        )
        .await
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        debug!("Binance Swap latest kline sync is driven by WebSocket");
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

struct BinanceSwapFundingRateGapSaver {
    repo: BinanceFundingRateRepository,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
    broadcast: bool,
    interval: HistoryInterval,
}

impl BinanceSwapFundingRateGapSaver {
    fn new(
        repo: BinanceFundingRateRepository,
        history_tx: mpsc::Sender<HistoryEntry>,
        inst_id: u64,
        broadcast: bool,
        interval: HistoryInterval,
    ) -> ShareHistoryBatchHandler<FundingRate> {
        Arc::new(Self {
            repo,
            history_tx,
            inst_id,
            broadcast,
            interval,
        }) as Arc<dyn HistoryBatchHandlerTrait<FundingRate> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<FundingRate> for BinanceSwapFundingRateGapSaver {
    async fn handle(&self, batch_data: Vec<FundingRate>) -> Result<(), ExchangeError> {
        // 因为原本的书简戳，会快几毫秒。所以这里做一点统。
        let funding_time = self.interval.get_now_close_unix_ms_utc();
        let funding_rates = batch_data
            .into_iter()
            .map(|rate| FundingRatePo::from_funding_rate(rate, self.inst_id, funding_time))
            .collect::<Vec<_>>();
        if let Err(error) = self.repo.batch_insert_funding_rate(funding_rates.clone()).await {
            error!("Error inserting Binance Swap Funding Rate into repository: {}", error);
        }
        if self.broadcast {
            for funding_rate in &funding_rates {
                if let Err(error) = self.history_tx.send(funding_rate.to_swap_history_entry()).await {
                    error!("Error broadcasting Binance Swap Funding Rate history entry: {}", error);
                }
            }
        }
        Ok(())
    }
}

pub struct BinanceSwapFundingRateSyncHistoryService {
    funding_rate_repo: BinanceFundingRateRepository,
    inst_provider: InstrumentProvider,
    binary_search_ds: BinarySearchDS,
    data_retention_ms: u64,
    history_tx: mpsc::Sender<HistoryEntry>,
}

impl BinanceSwapFundingRateSyncHistoryService {
    pub fn for_production(dash_board: Arc<BinanceDashboard>, data_retention_ms: u64, history_tx: mpsc::Sender<HistoryEntry>) -> Arc<Self> {
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::SwapFundingRate.table_name(),
            "funding_time".to_string(),
            "inst_id".to_string(),
        );
        Arc::new(Self {
            funding_rate_repo: get_default_funding_rate_repo(None, None),
            inst_provider: SwapInstrumentProvider::new_provider(dash_board),
            binary_search_ds,
            data_retention_ms,
            history_tx,
        })
    }

    async fn sync_funding_rate(&self, inst_id: u64, start_ms: u64, end_ms: u64, interval: HistoryInterval, broadcast: bool) -> Result<(), YuError> {
        let instrument = self
            .inst_provider
            .instruments(None)
            .into_iter()
            .find(|instrument| u64::try_from(instrument.id).ok() == Some(inst_id))
            .ok_or_else(|| YuError::new(&format!("Binance Swap instrument {inst_id} is unavailable for Funding Rate sync")))?;
        let request = CommonRequestBuilder::new(instrument.symbol, 1000, interval.clone());
        let saver = BinanceSwapFundingRateGapSaver::new(
            self.funding_rate_repo.clone(),
            self.history_tx.clone(),
            inst_id,
            broadcast,
            interval.clone(),
        );
        HistoryFetcherImpl::swap_funding_rate()
            .get_all_kline_data(request, Some(interval), Some(start_ms), Some(end_ms), Some(saver), true)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceSwapFundingRateSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let funding_rates = self.funding_rate_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if funding_rates.is_empty() {
            let _ = tx
                .send(Ok(ServerMessage {
                    history: vec![],
                    server_timestamp: unix_time_now_u64_utc(),
                }))
                .await;
            return Ok(());
        }

        for batch in funding_rates.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(FundingRatePo::to_swap_history_entry).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self, end_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let now_ms = end_ms.unwrap_or_else(|| unix_time_now_u64_utc());
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);
        let now_ms = interval.get_now_close_unix_ms_utc();
        let trading_instruments = self.inst_provider.instruments(None);
        let total = trading_instruments.len();
        let mut completed = 0;
        let mut no_gap_count = 0;
        let mut last_progress_log = tokio::time::Instant::now();
        for instrument in trading_instruments {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms < now_ms {
                let gaps_res = binary_search_gap(
                    instrument.id.to_string().as_str(),
                    "SWAP",
                    start_ms,
                    now_ms,
                    &interval,
                    self.binary_search_ds.clone(),
                    0,
                )?;
                let gaps = gaps_res.gaps;
                debug!("{} has {} gaps", instrument.symbol, gaps.len());
                no_gap_count = match gaps_res.level {
                    0 => no_gap_count + 1,
                    _ => no_gap_count,
                };
                for gap in gaps {
                    match gap {
                        ValidationGap::MissingData { start_time, end_time, .. } => {
                            self.sync_funding_rate(instrument.id as u64, start_time, end_time, HistoryInterval::OneHour, true)
                                .await?;
                        }
                        other => debug!("skip unsupported Binance Swap history gap: {:?}", other),
                    }
                }
            }

            completed += 1;
            if last_progress_log.elapsed() >= CHECK_HISTORY_PROGRESS_LOG_INTERVAL {
                info!(
                    "binance Swap Funding Rate history check progress: {}/{} instruments checked,{} instruments with gaps",
                    completed,
                    total,
                    completed - no_gap_count
                );
                last_progress_log = tokio::time::Instant::now();
            }
        }

        info!(
            "binance Swap Funding Rate history check done: {}/{} instruments checked,{} instruments with gaps",
            completed,
            total,
            completed - no_gap_count
        );
        Ok(())
    }

    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let start = interval.get_close_unix_ms(unix_time_now_u64_utc() - self.data_retention_ms);
        let instruments = self.inst_provider.instruments(None);
        let total = instruments.len();
        let end = interval.get_close_unix_ms(end_ms.unwrap_or_else(|| unix_time_now_u64_utc())) + 1;
        let mut last_progress_log = tokio::time::Instant::now();
        let mut completed = 0;
        let latest_timestamps = self.funding_rate_repo.inst_id_with_lastest_timestamp_before(end).await?;
        for instrument in instruments {
            // 需要到下一个周期
            let inst_start = latest_timestamps
                .get(&instrument.id)
                .cloned()
                .unwrap_or(start)
                .saturating_add(interval.to_milliseconds());
            if inst_start >= end || (end - inst_start) < interval.to_milliseconds() {
                debug!(
                    "Binance Swap Funding Rate for instrument {} (id: {}) is already up-to-date; skipping initial sync",
                    instrument.symbol, instrument.id
                );
                completed += 1;
                continue;
            }
            self.sync_funding_rate(instrument.id as u64, inst_start, end, interval.clone(), false)
                .await?;
            completed += 1;
            if last_progress_log.elapsed() >= INITIAL_HISTORY_PROGRESS_LOG_INTERVAL {
                info!(
                    "binance Swap Funding Rate history initialization progress: {}/{} instruments completed",
                    completed, total
                );
                last_progress_log = tokio::time::Instant::now();
            }
        }
        info!(
            "binance Swap Funding Rate history initialization done: {}/{} instruments completed",
            completed, total
        );
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        //因为时间并不是严格对齐的，所以这里需要多加5分钟，以免和之前数据对冲，以后如果修改interval，需要调节
        let start = interval.get_now_close_unix_ms_utc() - interval.to_milliseconds() + 5 * 60 * 1000;
        let end = interval.get_now_close_unix_ms_utc() + 30 * 1000;
        let instruments = self.inst_provider.instruments(None);
        for instrument in instruments {
            self.sync_funding_rate(instrument.id as u64, start, end, interval.clone(), true).await?;
        }
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

pub struct BinanceSyncInstrumentService {
    dashboard: Arc<BinanceDashboard>,
    instrument_repo: BNInstrumentRepository,
    option_instrument_service: Arc<BinanceOptionSyncInstrumentService>,
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
}

pub struct BinanceOptionSyncInstrumentService {
    instrument_repo: BinanceOptionInstrumentRepository,
    option_api: BinanceOptionExchangeApi,
    live_instruments: BinanceOptionInstrumentCache,
}

pub type BinanceOptionInstrumentCache = Arc<RwLock<HashMap<String, BinanceOptionInstrumentPo>>>;

impl BinanceOptionSyncInstrumentService {
    pub fn new(
        instrument_repo: BinanceOptionInstrumentRepository,
        option_api: BinanceOptionExchangeApi,
        live_instruments: BinanceOptionInstrumentCache,
    ) -> Self {
        Self {
            instrument_repo,
            option_api,
            live_instruments,
        }
    }

    pub fn for_production() -> Arc<Self> {
        Arc::new(Self::new(
            get_binance_option_instrument_repo(None),
            default_binance_option_api(),
            Arc::new(RwLock::new(HashMap::new())),
        ))
    }

    pub fn live_instruments(&self) -> BinanceOptionInstrumentCache {
        self.live_instruments.clone()
    }

    pub async fn list_all_instruments(&self) -> Result<Vec<BinanceOptionInstrumentPo>, YuError> {
        self.instrument_repo.list_all().await
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for BinanceOptionSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        self.list_all_instruments().await?.into_iter().map(option_instrument_to_proto).collect()
    }

    async fn sync_instrument(&self) -> Result<Vec<Instrument>, YuError> {
        let exchange_info = self.option_api.get_exchange_info().await?;
        let mut previous_instruments = self
            .instrument_repo
            .list_all()
            .await?
            .into_iter()
            .map(|instrument| (instrument.symbol.clone(), instrument))
            .collect::<HashMap<_, _>>();
        let mut current_instruments = HashMap::new();
        let mut protobuf_instruments = Vec::with_capacity(exchange_info.option_symbols.len());

        for symbol in &exchange_info.option_symbols {
            let instrument = match previous_instruments.remove(&symbol.symbol) {
                Some(previous) => BinanceOptionInstrumentPo::from_option_symbol_with_id(symbol, previous.id),
                None => BinanceOptionInstrumentPo::from(symbol),
            };
            let instrument = self.instrument_repo.upsert(instrument).await?;
            if instrument
                .status
                .eq_ignore_ascii_case(crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING)
            {
                current_instruments.insert(instrument.symbol.clone(), instrument.clone());
            }
            protobuf_instruments.push(option_instrument_to_proto(instrument)?);
        }

        for (_, mut instrument) in previous_instruments {
            if !instrument
                .status
                .eq_ignore_ascii_case(crate::binance::bn_consts::BN_SYMBOL_STATUS_NOT_TRADING)
            {
                instrument.status = crate::binance::bn_consts::BN_SYMBOL_STATUS_NOT_TRADING.to_string();
                self.instrument_repo.upsert(instrument).await?;
            }
        }

        *self.live_instruments.write().await = current_instruments;
        Ok(protobuf_instruments)
    }

    fn error_log(&self) -> String {
        "error when syncing Binance option instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<HistoryType, SyncHistoryService>, YuError> {
        Ok(HashMap::new())
    }
}

fn option_instrument_to_proto(instrument: BinanceOptionInstrumentPo) -> Result<Instrument, YuError> {
    instrument
        .into_proto_instrument()
        .map_err(|error| YuError::new(&format!("invalid Binance option instrument ID: {error}")))
}

pub struct BinanceOptionKlineSyncHistoryService {
    data_retention_ms: u64,
    live_instruments: BinanceOptionInstrumentCache,
    kline_repo: BinanceOptionKlineRepository,
    option_api: BinanceOptionExchangeApi,
    binary_search_ds: BinarySearchDS,
    history_tx: mpsc::Sender<HistoryEntry>,
    interval: HistoryInterval,
}

impl BinanceOptionKlineSyncHistoryService {
    pub fn new(
        data_retention_ms: u64,
        live_instruments: BinanceOptionInstrumentCache,
        kline_repo: BinanceOptionKlineRepository,
        option_api: BinanceOptionExchangeApi,
        binary_search_ds: BinarySearchDS,
        history_tx: mpsc::Sender<HistoryEntry>,
        interval: HistoryInterval,
    ) -> Arc<Self> {
        Arc::new(Self {
            data_retention_ms,
            live_instruments,
            kline_repo,
            option_api,
            binary_search_ds,
            history_tx,
            interval,
        })
    }

    pub fn for_production(
        data_retention_ms: u64,
        live_instruments: BinanceOptionInstrumentCache,
        history_tx: mpsc::Sender<HistoryEntry>,
        interval: HistoryInterval,
    ) -> Arc<Self> {
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::OptionKline.table_name(),
            "candle_begin_time".to_string(),
            "inst_id".to_string(),
        );
        Self::new(
            data_retention_ms,
            live_instruments,
            get_default_binance_option_kline_repo(None, None),
            default_binance_option_api(),
            binary_search_ds,
            history_tx,
            interval,
        )
    }

    async fn sync_instrument_range(
        &self,
        instrument: &BinanceOptionInstrumentPo,
        start_ms: u64,
        end_ms: u64,
        broadcast: bool,
    ) -> Result<(), YuError> {
        if start_ms >= end_ms {
            return Ok(());
        }
        let request_end_ms = end_ms.saturating_sub(1);
        let response = self
            .option_api
            .get_all_klines(instrument.symbol.clone(), self.interval.as_ref().to_string(), start_ms, request_end_ms)
            .await?;
        let instrument_id = u64::try_from(instrument.id).map_err(|error| YuError::new(&format!("invalid Binance option instrument ID: {error}")))?;
        let klines = response
            .into_iter()
            .map(|kline| BinanceOptionKlinePo::from_option_kline(instrument_id, &kline))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|kline| kline.candle_begin_time >= start_ms && kline.candle_begin_time < end_ms)
            .collect::<Vec<_>>();

        if klines.is_empty() {
            return Ok(());
        }
        self.kline_repo.batch_insert_kline(klines.clone()).await?;
        if broadcast {
            for kline in klines {
                self.history_tx
                    .send(kline.to_history_entry())
                    .await
                    .map_err(|error| YuError::CustomError(format!("failed to broadcast Binance option Kline history: {error}")))?;
            }
        }
        Ok(())
    }

    async fn live_instruments(&self) -> Vec<BinanceOptionInstrumentPo> {
        self.live_instruments.read().await.values().cloned().collect()
    }

    fn aligned_end(&self, requested_end_ms: Option<u64>) -> u64 {
        let now_ms = unix_time_now_u64_utc();
        self.interval.get_close_unix_ms(requested_end_ms.unwrap_or(now_ms).min(now_ms))
    }

    fn retention_start(&self, end_ms: u64) -> u64 {
        self.interval.get_close_unix_ms(end_ms.saturating_sub(self.data_retention_ms))
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceOptionKlineSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let klines = self.kline_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if klines.is_empty() {
            tx.send(Ok(ServerMessage {
                history: Vec::new(),
                server_timestamp: unix_time_now_u64_utc(),
            }))
            .await
            .map_err(|error| YuError::CustomError(format!("failed to return empty Binance option Kline history: {error}")))?;
            return Ok(());
        }

        for batch in klines.chunks(batch_size.max(1)) {
            tx.send(Ok(ServerMessage {
                history: batch.iter().map(BinanceOptionKlinePo::to_history_entry).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            }))
            .await
            .map_err(|error| YuError::CustomError(format!("failed to return Binance option Kline history: {error}")))?;
        }
        Ok(())
    }

    async fn check_history_data(&self, end_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        let end_ms = self.aligned_end(end_ms);
        let start_ms = self.retention_start(end_ms);
        let instruments = self.live_instruments().await;
        for instrument in instruments {
            let instrument_id =
                u64::try_from(instrument.id).map_err(|error| YuError::new(&format!("invalid Binance option instrument ID: {error}")))?;
            let gaps = binary_search_gap(
                &instrument_id.to_string(),
                "BINANCE_OPTION",
                start_ms,
                end_ms,
                &self.interval,
                self.binary_search_ds.clone(),
                0,
            )?
            .gaps;
            for gap in gaps {
                if let ValidationGap::MissingData { start_time, end_time, .. } = gap {
                    self.sync_instrument_range(&instrument, start_time, end_time, true).await?;
                }
            }
        }
        Ok(())
    }

    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let end_ms = self.aligned_end(end_ms);
        let retention_start_ms = self.retention_start(end_ms);
        let latest_timestamps = self.kline_repo.inst_id_with_latest_timestamp_before(end_ms).await?;
        for instrument in self.live_instruments().await {
            let start_ms = latest_timestamps
                .get(&instrument.id)
                .map(|latest| latest.saturating_add(self.interval.to_milliseconds()))
                .unwrap_or(retention_start_ms)
                .max(retention_start_ms);
            self.sync_instrument_range(&instrument, start_ms, end_ms, false).await?;
        }
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        let end_ms = self.aligned_end(None);
        let retention_start_ms = self.retention_start(end_ms);
        let latest_timestamps = self.kline_repo.inst_id_with_latest_timestamp_before(end_ms).await?;
        for instrument in self.live_instruments().await {
            let start_ms = latest_timestamps
                .get(&instrument.id)
                .map(|latest| latest.saturating_add(self.interval.to_milliseconds()))
                .unwrap_or(retention_start_ms)
                .max(retention_start_ms);
            self.sync_instrument_range(&instrument, start_ms, end_ms, true).await?;
        }
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

pub struct BinanceOptionMarketPriceSyncHistoryService {
    live_instruments: BinanceOptionInstrumentCache,
    market_price_repo: BinanceOptionMarketPriceRepository,
    option_api: BinanceOptionExchangeApi,
    history_tx: mpsc::Sender<HistoryEntry>,
}

impl BinanceOptionMarketPriceSyncHistoryService {
    pub fn new(
        live_instruments: BinanceOptionInstrumentCache,
        market_price_repo: BinanceOptionMarketPriceRepository,
        option_api: BinanceOptionExchangeApi,
        history_tx: mpsc::Sender<HistoryEntry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            live_instruments,
            market_price_repo,
            option_api,
            history_tx,
        })
    }

    pub fn for_production(live_instruments: BinanceOptionInstrumentCache, history_tx: mpsc::Sender<HistoryEntry>) -> Arc<Self> {
        Self::new(
            live_instruments,
            get_default_binance_option_market_price_repo(None, None),
            default_binance_option_api(),
            history_tx,
        )
    }

    async fn capture_market_prices(&self, broadcast: bool) -> Result<(), YuError> {
        let instruments = self.live_instruments.read().await.clone();
        let response = self.option_api.get_all_market_price().await?;
        let mut prices = Vec::new();
        for price in response {
            let Some(instrument) = instruments.get(&price.symbol) else {
                debug!("skip Binance option market price for unknown or non-trading instrument {}", price.symbol);
                continue;
            };
            let instrument_id =
                u64::try_from(instrument.id).map_err(|error| YuError::new(&format!("invalid Binance option instrument ID: {error}")))?;
            prices.push(BinanceOptionMarketPricePo::from_market_price(instrument_id, &price));
        }
        if prices.is_empty() {
            return Ok(());
        }
        self.market_price_repo.batch_insert_market_price(prices.clone()).await?;
        if broadcast {
            for price in prices {
                self.history_tx
                    .send(price.to_history_entry())
                    .await
                    .map_err(|error| YuError::CustomError(format!("failed to broadcast Binance option market price history: {error}")))?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceOptionMarketPriceSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let prices = self.market_price_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if prices.is_empty() {
            tx.send(Ok(ServerMessage {
                history: Vec::new(),
                server_timestamp: unix_time_now_u64_utc(),
            }))
            .await
            .map_err(|error| YuError::CustomError(format!("failed to return empty Binance option market price history: {error}")))?;
            return Ok(());
        }

        for batch in prices.chunks(batch_size.max(1)) {
            tx.send(Ok(ServerMessage {
                history: batch.iter().map(BinanceOptionMarketPricePo::to_history_entry).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            }))
            .await
            .map_err(|error| YuError::CustomError(format!("failed to return Binance option market price history: {error}")))?;
        }
        Ok(())
    }

    async fn check_history_data(&self, _end_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        debug!("skip Binance option market price gap check: REST API exposes only current prices, not historical snapshots");
        Ok(())
    }

    async fn initial_history_data(&self, _end_ms: Option<u64>) -> Result<(), YuError> {
        debug!("skip Binance option market price gap check: REST API exposes only current prices, not historical snapshots");
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        self.capture_market_prices(true).await
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

impl BinanceSyncInstrumentService {
    pub fn for_production(dashboard: Arc<BinanceDashboard>, support_history_service: HashMap<HistoryType, SyncHistoryService>) -> Arc<Self> {
        Self::for_production_with_option_service(dashboard, support_history_service, BinanceOptionSyncInstrumentService::for_production())
    }

    pub fn for_production_with_option_service(
        dashboard: Arc<BinanceDashboard>,
        support_history_service: HashMap<HistoryType, SyncHistoryService>,
        option_instrument_service: Arc<BinanceOptionSyncInstrumentService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            dashboard,
            instrument_repo: get_instrument_repo_for_production(None),
            option_instrument_service,
            support_history_service,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for BinanceSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        let mut instruments = self
            .instrument_repo
            .get_trading_instrument_by_type(InstrumentType::Spot)
            .await?
            .into_iter()
            .map(BinanceInstrument::into_proto_instrument)
            .collect::<Vec<_>>();
        instruments.extend(
            self.instrument_repo
                .get_trading_instrument_by_type(InstrumentType::Swap)
                .await?
                .into_iter()
                .map(BinanceInstrument::into_proto_instrument),
        );
        instruments.extend(self.option_instrument_service.list_instruments().await?);
        Ok(instruments)
    }

    async fn sync_instrument(&self) -> Result<Vec<Instrument>, YuError> {
        let snapshot = self.dashboard.sync_instruments().await?;
        let mut instruments = snapshot
            .live_spot
            .clone()
            .into_values()
            .map(BinanceInstrument::into_proto_instrument)
            .collect::<Vec<_>>();
        instruments.extend(snapshot.live_swap.clone().into_values().map(BinanceInstrument::into_proto_instrument));
        instruments.extend(self.option_instrument_service.sync_instrument().await?);
        Ok(instruments)
    }

    fn error_log(&self) -> String {
        "error when syncing Binance instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<HistoryType, SyncHistoryService>, YuError> {
        Ok(self.support_history_service.clone())
    }
}

#[cfg(test)]
mod option_instrument_tests {
    use super::*;
    use crate::binance::db_consts::CREATE_BINANCE_OPTION_INSTRUMENTS_TABLE;
    use crate::binance::duckdb_repository::get_binance_option_instrument_repo;
    use crate::sync::models::grpc_sync::{InstrumentType as SyncInstrumentType, instrument};
    use crate::test_utils::create_memory_db_provider;
    use std::sync::Mutex;
    use yue::binance::bn_models::option_restful::{KLinesResponse, MarketPriceResponse, OptionExchangeInfo, OptionSymbol, OptionSymbolFilter};
    use yue::binance::option_api::BinanceOptionApiTrait;
    use yue::query_message::DataSourceProviderTrait;

    struct TestOptionApi {
        exchange_info: Mutex<Option<OptionExchangeInfo>>,
    }

    impl TestOptionApi {
        fn new(exchange_info: Option<OptionExchangeInfo>) -> Self {
            Self {
                exchange_info: Mutex::new(exchange_info),
            }
        }
    }

    #[cfg(test)]
    mod option_history_service_tests {
        use super::*;
        use crate::binance::duckdb_repository::{MockBinanceOptionKlineRepositoryTrait, MockBinanceOptionMarketPriceRepositoryTrait};
        use crate::data_integrity::check::MockBinarySearchDSTrait;
        use crate::sync::models::grpc_sync::{HistoryType, history_entry};
        use rust_decimal::Decimal;
        use yue::binance::bn_models::option_restful::{KLinesResponse, MarketPriceResponse, OptionExchangeInfo};
        use yue::binance::option_api::BinanceOptionApiTrait;

        struct OptionHistoryTestApi {
            market_prices: String,
            fail_kline: bool,
            fail_market_price: bool,
        }

        impl OptionHistoryTestApi {
            fn new() -> Self {
                Self {
                    market_prices: r#"[
                            {
                                "symbol":"BTC-270101-50000-C",
                                "markPrice":"123.000000000000000001",
                                "bidIV":"0.21",
                                "askIV":"0.22",
                                "markIV":"0.215",
                                "delta":"0.5",
                                "theta":"-0.01",
                                "gamma":"0.000000000000000123",
                                "vega":"0.03",
                                "highPriceLimit":"130.1",
                                "lowPriceLimit":"110.2",
                                "riskFreeInterest":"0.1"
                            },
                            {
                                "symbol":"UNKNOWN-270101-50000-C",
                                "markPrice":"123",
                                "bidIV":"0.21",
                                "askIV":"0.22",
                                "markIV":"0.215",
                                "delta":"0.5",
                                "theta":"-0.01",
                                "gamma":"0.0001",
                                "vega":"0.03",
                                "highPriceLimit":"130.1",
                                "lowPriceLimit":"110.2",
                                "riskFreeInterest":"0.1"
                            }
                        ]"#
                    .to_string(),
                    fail_kline: false,
                    fail_market_price: false,
                }
            }

            fn kline_json(start_ms: u64, interval_ms: u64) -> String {
                serde_json::json!([[
                    start_ms,
                    "100.000000000000000001",
                    "101.2",
                    "99.8",
                    "100.5",
                    "2.25",
                    start_ms + interval_ms - 1,
                    "225.000000000000000002",
                    7,
                    "1.25",
                    "125.000000000000000003",
                    0
                ]])
                .to_string()
            }
        }

        #[async_trait]
        impl BinanceOptionApiTrait for OptionHistoryTestApi {
            async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, ExchangeError> {
                Err(ExchangeError::new("exchange-info endpoint is not used by history tests"))
            }

            async fn get_kline_history(&self, request: CommonRequestBuilder) -> Result<KLinesResponse, ExchangeError> {
                if self.fail_kline {
                    return Err(ExchangeError::new("test Kline REST failure"));
                }
                let start_ms = request.start_time.unwrap_or_default();
                let interval_ms = request.interval.as_ref().unwrap().to_milliseconds();
                serde_json::from_str(&Self::kline_json(start_ms, interval_ms))
                    .map_err(|error| ExchangeError::new(&format!("invalid test Kline JSON: {error}")))
            }

            async fn get_all_market_price(&self) -> Result<MarketPriceResponse, ExchangeError> {
                if self.fail_market_price {
                    return Err(ExchangeError::new("test market-price REST failure"));
                }
                serde_json::from_str(&self.market_prices).map_err(|error| ExchangeError::new(&format!("invalid test market-price JSON: {error}")))
            }
        }

        fn option_instrument(id: i64, symbol: &str) -> BinanceOptionInstrumentPo {
            BinanceOptionInstrumentPo {
                id,
                expiry_date: 1_800_000_000_000,
                symbol: symbol.to_string(),
                side: "CALL".to_string(),
                strike_price: Decimal::ONE,
                underlying: "BTCUSDT".to_string(),
                unit: 1,
                liquidation_fee_rate: Decimal::ZERO,
                min_qty: Decimal::ONE,
                max_qty: Decimal::ONE,
                initial_margin: Decimal::ZERO,
                maintenance_margin: Decimal::ZERO,
                min_initial_margin: Decimal::ZERO,
                min_maintenance_margin: Decimal::ZERO,
                price_scale: 2,
                quantity_scale: 2,
                quote_asset: "USDT".to_string(),
                contract_type: "CRYPTO_OPTIONS".to_string(),
                underlying_type: "CRYPTO".to_string(),
                naked_sell: None,
                status: "TRADING".to_string(),
                filters: Vec::new(),
            }
        }

        fn option_instrument_cache() -> BinanceOptionInstrumentCache {
            Arc::new(RwLock::new(HashMap::from([(
                "BTC-270101-50000-C".to_string(),
                option_instrument(42, "BTC-270101-50000-C"),
            )])))
        }

        fn test_kline_service(
            repository: MockBinanceOptionKlineRepositoryTrait,
            api: OptionHistoryTestApi,
            cache: BinanceOptionInstrumentCache,
            history_tx: mpsc::Sender<HistoryEntry>,
            retention_ms: u64,
            interval: HistoryInterval,
            binary_search_ds: Option<BinarySearchDS>,
        ) -> Arc<BinanceOptionKlineSyncHistoryService> {
            let binary_search_ds = binary_search_ds.unwrap_or_else(|| Arc::new(MockBinarySearchDSTrait::new()));
            BinanceOptionKlineSyncHistoryService::new(
                retention_ms,
                cache,
                Arc::new(repository),
                Arc::new(api),
                binary_search_ds,
                history_tx,
                interval,
            )
        }

        // 测试目的：验证期权 Kline REST 数据通过关联的 BinanceOptionInstrument ID 持久化并广播。
        // 准备数据/过程：mock 返回一个完整 K 线，注入含对应期权 instrument 的缓存和 repository mock，调用区间同步。
        // 预期结果：保存 PO 使用 instrument ID 42 且 Decimal 精度不变；广播条目使用 BinanceOptionKline 历史类型。
        #[tokio::test]
        async fn option_kline_rest_sync_persists_and_broadcasts_by_option_instrument() {
            let start_ms = 1_700_000_000_000;
            let interval = HistoryInterval::OneMinute;
            let mut repository = MockBinanceOptionKlineRepositoryTrait::new();
            repository
                .expect_batch_insert_kline()
                .times(1)
                .withf(|klines| {
                    klines.len() == 1
                        && klines[0].inst_id == 42
                        && klines[0].open.to_string() == "100.000000000000000001"
                        && klines[0].quote_asset_volume.to_string() == "225.000000000000000002"
                })
                .returning(|_| Ok(()));
            let (history_tx, mut history_rx) = mpsc::channel(4);
            let service = test_kline_service(
                repository,
                OptionHistoryTestApi::new(),
                option_instrument_cache(),
                history_tx,
                60_000,
                interval.clone(),
                None,
            );

            service
                .sync_instrument_range(
                    &option_instrument(42, "BTC-270101-50000-C"),
                    start_ms,
                    start_ms + interval.to_milliseconds(),
                    true,
                )
                .await
                .unwrap();

            let entry = history_rx.recv().await.unwrap();
            assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceOptionKline);
            assert_eq!(entry.inst_id, 42);
            let Some(history_entry::Payload::BinanceOptionKline(kline)) = entry.payload else {
                panic!("expected Binance Option Kline payload");
            };
            assert_eq!(kline.candle_begin_time, start_ms);
            assert_eq!(kline.open, "100.000000000000000001");
            assert_eq!(kline.close_time, start_ms + interval.to_milliseconds() - 1);
        }

        // 测试目的：验证期权 Kline 的完整性检查会通过 REST 修复缺失区间并广播修复数据。
        // 准备数据/过程：mock 二分检查报告一个缺失周期，API 根据请求区间返回对应 K 线，并由 repository 接收批量保存。
        // 预期结果：缺口记录成功保存并以 BinanceOptionKline payload 广播，instrument 与周期时间正确。
        #[tokio::test]
        async fn option_kline_history_check_repairs_and_broadcasts_missing_candles() {
            let interval = HistoryInterval::OneHour;
            let mut binary_search = MockBinarySearchDSTrait::new();
            binary_search.expect_count_distinct_between().returning(|_, _, _| Ok(0));
            binary_search.expect_table_name().return_const("bn_option_kline".to_string());
            let mut repository = MockBinanceOptionKlineRepositoryTrait::new();
            repository
                .expect_batch_insert_kline()
                .times(1)
                .withf(|klines| klines.len() == 1 && klines[0].inst_id == 42)
                .returning(|_| Ok(()));
            let (history_tx, mut history_rx) = mpsc::channel(4);
            let service = test_kline_service(
                repository,
                OptionHistoryTestApi::new(),
                option_instrument_cache(),
                history_tx,
                interval.to_milliseconds(),
                interval.clone(),
                Some(Arc::new(binary_search)),
            );
            let end_ms = interval.get_now_close_unix_ms_utc();

            service.check_history_data(Some(end_ms)).await.unwrap();

            let entry = history_rx.recv().await.unwrap();
            assert_eq!(entry.inst_id, 42);
            let Some(history_entry::Payload::BinanceOptionKline(kline)) = entry.payload else {
                panic!("expected repaired Binance Option Kline payload");
            };
            assert_eq!(kline.candle_begin_time, end_ms - interval.to_milliseconds());
        }

        // 测试目的：验证期权 Kline 初始回补只写入数据库，不向客户端广播。
        // 准备数据/过程：使用空的最新时间戳结果和一个活跃期权 instrument，调用 initial_history_data 并读取同步 channel。
        // 预期结果：回补数据通过批量 repository 保存，HistoryEntry channel 保持为空。
        #[tokio::test]
        async fn option_kline_initial_sync_persists_without_broadcast() {
            let interval = HistoryInterval::OneMinute;
            let mut repository = MockBinanceOptionKlineRepositoryTrait::new();
            repository
                .expect_inst_id_with_latest_timestamp_before()
                .times(1)
                .returning(|_| Ok(HashMap::new()));
            repository
                .expect_batch_insert_kline()
                .times(1)
                .withf(|klines| klines.len() == 1 && klines[0].inst_id == 42)
                .returning(|_| Ok(()));
            let (history_tx, mut history_rx) = mpsc::channel(4);
            let service = test_kline_service(
                repository,
                OptionHistoryTestApi::new(),
                option_instrument_cache(),
                history_tx,
                interval.to_milliseconds(),
                interval.clone(),
                None,
            );
            let end_ms = interval.get_now_close_unix_ms_utc();

            service.initial_history_data(Some(end_ms)).await.unwrap();

            assert!(history_rx.try_recv().is_err());
        }

        // 测试目的：验证期权 Kline 查询历史按 batch_size 返回服务端存储数据。
        // 准备数据/过程：mock repository 返回一条 K 线，以 batch_size=1 调用 query_and_broadcast_history 并读取 ServerMessage。
        // 预期结果：返回一条正确的 BinanceOptionKline payload，且历史类型、instrument 和字段均保留。
        #[tokio::test]
        async fn option_kline_query_broadcasts_repository_history() {
            let kline = BinanceOptionKlinePo {
                id: 8,
                inst_id: 42,
                candle_begin_time: 1_700_000_000_000,
                open: "100.000000000000000001".parse().unwrap(),
                high: "101.2".parse().unwrap(),
                low: "99.8".parse().unwrap(),
                close: "100.5".parse().unwrap(),
                volume: "2.25".parse().unwrap(),
                close_time: 1_700_000_059_999,
                quote_asset_volume: "225.000000000000000002".parse().unwrap(),
                number_of_trades: 7,
                taker_buy_base_asset_volume: "1.25".parse().unwrap(),
                taker_buy_quote_asset_volume: "125.000000000000000003".parse().unwrap(),
            };
            let mut repository = MockBinanceOptionKlineRepositoryTrait::new();
            repository
                .expect_find_by_inst_id_between()
                .withf(|inst_id, start_ms, end_ms| *inst_id == 42 && *start_ms == 0 && *end_ms == 2_000_000_000_000)
                .times(1)
                .returning(move |_, _, _| Ok(vec![kline.clone()]));
            let (history_tx, _) = mpsc::channel(1);
            let service = test_kline_service(
                repository,
                OptionHistoryTestApi::new(),
                option_instrument_cache(),
                history_tx,
                0,
                HistoryInterval::OneMinute,
                None,
            );
            let (tx, mut rx) = mpsc::channel(2);

            service.query_and_broadcast_history(42, 0, 2_000_000_000_000, 1, tx).await.unwrap();

            let message = rx.recv().await.unwrap().unwrap();
            assert_eq!(message.history.len(), 1);
            assert_eq!(
                HistoryType::try_from(message.history[0].history_type).unwrap(),
                HistoryType::BinanceOptionKline
            );
            assert_eq!(message.history[0].inst_id, 42);
        }

        // 测试目的：验证期权 Kline REST 请求或本地批量保存失败时同步服务显式返回错误。
        // 准备数据/过程：分别使用失败 REST mock 和失败 repository mock 调用同一 K 线区间同步。
        // 预期结果：两种失败均向调用方返回 Err，且 REST 失败时不尝试保存或广播。
        #[tokio::test]
        async fn option_kline_sync_propagates_rest_and_repository_errors() {
            let start_ms = 1_700_000_000_000;
            let interval = HistoryInterval::OneMinute;
            let (history_tx, _) = mpsc::channel(1);
            let api_error_service = test_kline_service(
                MockBinanceOptionKlineRepositoryTrait::new(),
                OptionHistoryTestApi {
                    fail_kline: true,
                    ..OptionHistoryTestApi::new()
                },
                option_instrument_cache(),
                history_tx.clone(),
                60_000,
                interval.clone(),
                None,
            );
            assert!(
                api_error_service
                    .sync_instrument_range(
                        &option_instrument(42, "BTC-270101-50000-C"),
                        start_ms,
                        start_ms + interval.to_milliseconds(),
                        true,
                    )
                    .await
                    .is_err()
            );

            let mut repository = MockBinanceOptionKlineRepositoryTrait::new();
            repository
                .expect_batch_insert_kline()
                .times(1)
                .returning(|_| Err(YuError::new("test Kline repository failure")));
            let repository_error_service = test_kline_service(
                repository,
                OptionHistoryTestApi::new(),
                option_instrument_cache(),
                history_tx,
                60_000,
                interval.clone(),
                None,
            );
            assert!(
                repository_error_service
                    .sync_instrument_range(
                        &option_instrument(42, "BTC-270101-50000-C"),
                        start_ms,
                        start_ms + interval.to_milliseconds(),
                        true,
                    )
                    .await
                    .is_err()
            );
        }

        // 测试目的：验证 Market Price 的初始 REST 快照只落库，后续 sync_all 才广播已匹配的 option instrument。
        // 准备数据/过程：API mock 返回一个已知 symbol 和一个未知 symbol，调用初始同步、周期同步及无历史端点的检查。
        // 预期结果：两次保存均只包含 instrument ID 42 的匹配记录，初始同步无广播，sync_all 发送正确 Market Price 条目。
        #[tokio::test]
        async fn option_market_price_sync_links_instruments_and_suppresses_initial_broadcast() {
            let mut repository = MockBinanceOptionMarketPriceRepositoryTrait::new();
            repository
                .expect_batch_insert_market_price()
                .times(1)
                .withf(|prices| {
                    prices.len() == 1
                        && prices[0].inst_id == 42
                        && prices[0].symbol == "BTC-270101-50000-C"
                        && prices[0].mark_price.to_string() == "123.000000000000000001"
                })
                .returning(|_| Ok(()));
            let (history_tx, mut history_rx) = mpsc::channel(4);
            let service = BinanceOptionMarketPriceSyncHistoryService::new(
                option_instrument_cache(),
                Arc::new(repository),
                Arc::new(OptionHistoryTestApi::new()),
                history_tx,
            );

            service.initial_history_data(None).await.unwrap();
            assert!(history_rx.try_recv().is_err());
            service.sync_all().await.unwrap();

            let entry = history_rx.recv().await.unwrap();
            assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceOptionMarketPrice);
            assert_eq!(entry.inst_id, 42);
            let Some(history_entry::Payload::BinanceOptionMarketPrice(price)) = entry.payload else {
                panic!("expected Binance Option Market Price payload");
            };
            assert_eq!(price.symbol, "BTC-270101-50000-C");
            assert_eq!(price.mark_price, "123.000000000000000001");
            assert!(history_rx.try_recv().is_err());
        }

        // 测试目的：验证 Market Price 历史查询使用 repository 结果并按 batch_size 发送。
        // 准备数据/过程：mock repository 返回一条价格快照，调用 query_and_broadcast_history 并读取返回消息。
        // 预期结果：ServerMessage 中包含一条关联 instrument ID 42 的 BinanceOptionMarketPrice。
        #[tokio::test]
        async fn option_market_price_query_broadcasts_repository_history() {
            let price = BinanceOptionMarketPricePo {
                id: 8,
                inst_id: 42,
                symbol: "BTC-270101-50000-C".to_string(),
                acquire_ts: 1_700_000_000_123,
                mark_price: "123.000000000000000001".parse().unwrap(),
                bid_iv: "0.21".parse().unwrap(),
                ask_iv: "0.22".parse().unwrap(),
                mark_iv: "0.215".parse().unwrap(),
                delta: "0.5".parse().unwrap(),
                theta: "-0.01".parse().unwrap(),
                gamma: "0.000000000000000123".parse().unwrap(),
                vega: "0.03".parse().unwrap(),
                high_price_limit: "130.1".parse().unwrap(),
                low_price_limit: "110.2".parse().unwrap(),
                risk_free_interest: "0.1".parse().unwrap(),
            };
            let mut repository = MockBinanceOptionMarketPriceRepositoryTrait::new();
            repository
                .expect_find_by_inst_id_between()
                .withf(|inst_id, start_ms, end_ms| *inst_id == 42 && *start_ms == 0 && *end_ms == 2_000_000_000_000)
                .times(1)
                .returning(move |_, _, _| Ok(vec![price.clone()]));
            let (history_tx, _) = mpsc::channel(1);
            let service = BinanceOptionMarketPriceSyncHistoryService::new(
                option_instrument_cache(),
                Arc::new(repository),
                Arc::new(OptionHistoryTestApi::new()),
                history_tx,
            );
            let (tx, mut rx) = mpsc::channel(2);

            service.query_and_broadcast_history(42, 0, 2_000_000_000_000, 1, tx).await.unwrap();

            let message = rx.recv().await.unwrap().unwrap();
            assert_eq!(message.history.len(), 1);
            assert_eq!(
                HistoryType::try_from(message.history[0].history_type).unwrap(),
                HistoryType::BinanceOptionMarketPrice
            );
            assert_eq!(message.history[0].inst_id, 42);
        }

        // 测试目的：验证 Binance Option REST API 失败时 Market Price 服务向调用方显式返回错误。
        // 准备数据/过程：注入返回 REST 错误的 API mock，分别调用初始快照与周期同步。
        // 预期结果：两种同步入口均返回 Err，不将 API 故障伪装成无数据成功。
        #[tokio::test]
        async fn option_market_price_sync_propagates_rest_errors() {
            let api = OptionHistoryTestApi {
                fail_market_price: true,
                ..OptionHistoryTestApi::new()
            };
            let mut repository = MockBinanceOptionMarketPriceRepositoryTrait::new();
            repository.expect_batch_insert_market_price().times(0);
            let (history_tx, _) = mpsc::channel(1);
            let service = BinanceOptionMarketPriceSyncHistoryService::new(option_instrument_cache(), Arc::new(repository), Arc::new(api), history_tx);

            assert!(service.sync_all().await.is_err());
        }
    }

    #[async_trait]
    impl BinanceOptionApiTrait for TestOptionApi {
        async fn get_exchange_info(&self) -> Result<OptionExchangeInfo, ExchangeError> {
            self.exchange_info
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| ExchangeError::new("test exchange info unavailable"))
        }

        async fn get_kline_history(&self, _request_builder: CommonRequestBuilder) -> Result<KLinesResponse, ExchangeError> {
            Err(ExchangeError::new("option kline API is not used by instrument tests"))
        }

        async fn get_all_market_price(&self) -> Result<MarketPriceResponse, ExchangeError> {
            Err(ExchangeError::new("option market price API is not used by instrument tests"))
        }
    }

    fn option_symbol(status: &str) -> OptionSymbol {
        OptionSymbol {
            expiry_date: 1_700_000_000_123,
            filters: vec![OptionSymbolFilter {
                filter_type: "PRICE_FILTER".to_string(),
                min_price: Some("0.00000001".parse().unwrap()),
                max_price: Some("80000.01000000".parse().unwrap()),
                tick_size: Some("0.01000000".parse().unwrap()),
                min_qty: Some("0.01".parse().unwrap()),
                max_qty: None,
                step_size: Some("0.001".parse().unwrap()),
            }],
            symbol: "BTC-270101-50000-C".to_string(),
            side: "CALL".to_string(),
            strike_price: "50000.00000001".parse().unwrap(),
            underlying: "BTCUSDT".to_string(),
            unit: 1,
            liquidation_fee_rate: "0.0019000".parse().unwrap(),
            min_qty: "0.01".parse().unwrap(),
            max_qty: "100".parse().unwrap(),
            initial_margin: "0.15".parse().unwrap(),
            maintenance_margin: "0.075".parse().unwrap(),
            min_initial_margin: "0.1".parse().unwrap(),
            min_maintenance_margin: "0.05".parse().unwrap(),
            price_scale: 2,
            quantity_scale: 2,
            quote_asset: "USDT".to_string(),
            contract_type: "CRYPTO_OPTIONS".to_string(),
            underlying_type: "CRYPTO".to_string(),
            naked_sell: Some(false),
            status: status.to_string(),
        }
    }

    fn option_exchange_info(symbol: OptionSymbol) -> OptionExchangeInfo {
        OptionExchangeInfo {
            timezone: "UTC".to_string(),
            server_time: 1_700_000_000_000,
            option_contracts: Vec::new(),
            option_assets: Vec::new(),
            option_symbols: vec![symbol],
            rate_limits: Vec::new(),
        }
    }

    fn option_instrument_service(
        exchange_info: Option<OptionExchangeInfo>,
    ) -> (
        BinanceOptionSyncInstrumentService,
        Arc<RwLock<HashMap<String, BinanceOptionInstrumentPo>>>,
        BinanceOptionInstrumentRepository,
    ) {
        let provider = create_memory_db_provider();
        let conn = provider.acquire().unwrap();
        for statement in CREATE_BINANCE_OPTION_INSTRUMENTS_TABLE
            .split(';')
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
        {
            conn.execute(statement, []).unwrap();
        }
        drop(conn);

        let instrument_repo = get_binance_option_instrument_repo(Some(provider));
        let api: BinanceOptionExchangeApi = Arc::new(TestOptionApi::new(exchange_info));
        let live_instruments = Arc::new(RwLock::new(HashMap::new()));
        (
            BinanceOptionSyncInstrumentService::new(instrument_repo.clone(), api, live_instruments.clone()),
            live_instruments,
            instrument_repo,
        )
    }

    // 测试目的：验证 Option instrument service 从交易所 API 同步、持久化、标记缺失记录并缓存当前 TRADING instrument。
    // 准备数据/过程：预存同 symbol 的历史 ID 和另一条旧 instrument，再注入返回当前 symbol 的 API mock，调用同步并查询列表、状态与缓存。
    // 预期结果：同 symbol 保留原服务器 ID，缺失记录变为 NOT_TRADING，proto 使用 BinanceOption 类型，live cache 只包含当前记录且无历史类型。
    #[tokio::test]
    async fn option_instrument_service_syncs_persists_and_lists_api_symbols() {
        let (service, live_instruments, instrument_repo) = option_instrument_service(Some(option_exchange_info(option_symbol("TRADING"))));

        assert!(service.list_instruments().await.unwrap().is_empty());
        let current_symbol = option_symbol("TRADING");
        instrument_repo
            .upsert(BinanceOptionInstrumentPo::from_option_symbol_with_id(&current_symbol, 123))
            .await
            .unwrap();
        let mut removed_symbol = option_symbol("TRADING");
        removed_symbol.symbol = "BTC-270101-40000-C".to_string();
        instrument_repo
            .upsert(BinanceOptionInstrumentPo::from_option_symbol_with_id(&removed_symbol, 456))
            .await
            .unwrap();

        let synced = service.sync_instrument().await.unwrap();
        let listed = service.list_instruments().await.unwrap();

        assert_eq!(synced.len(), 1);
        assert_eq!(listed.len(), 2);
        assert_eq!(synced[0].instrument_type, SyncInstrumentType::BinanceOption as i32);
        let Some(instrument::Payload::BinanceOptionInstrument(payload)) = synced[0].payload.as_ref() else {
            panic!("expected BinanceOptionInstrument payload");
        };
        assert_eq!(payload.symbol, "BTC-270101-50000-C");
        assert_eq!(payload.server_id, 123);
        assert_eq!(live_instruments.read().await.len(), 1);
        let stored_instruments = instrument_repo.list_all().await.unwrap();
        assert_eq!(
            stored_instruments
                .iter()
                .find(|instrument| instrument.symbol == "BTC-270101-40000-C")
                .unwrap()
                .status,
            crate::binance::bn_consts::BN_SYMBOL_STATUS_NOT_TRADING
        );
        assert!(service.support_history_list().unwrap().is_empty());
        assert_eq!(service.error_log(), "error when syncing Binance option instruments");
    }

    // 测试目的：验证交易所 API 获取失败时 Option instrument service 显式返回错误且不覆盖 live cache。
    // 准备数据/过程：注入不提供 exchange info 的 API mock，并预置一条 live instrument 后调用 sync_instrument。
    // 预期结果：同步返回 Err，且之前缓存的 instrument 保持不变。
    #[tokio::test]
    async fn option_instrument_service_propagates_api_errors_without_clearing_cache() {
        let (service, live_instruments, _) = option_instrument_service(None);
        live_instruments.write().await.insert(
            "BTC-OLD-C".to_string(),
            BinanceOptionInstrumentPo {
                id: 1,
                expiry_date: 1,
                symbol: "BTC-OLD-C".to_string(),
                side: "CALL".to_string(),
                strike_price: "1".parse().unwrap(),
                underlying: "BTCUSDT".to_string(),
                unit: 1,
                liquidation_fee_rate: "0".parse().unwrap(),
                min_qty: "1".parse().unwrap(),
                max_qty: "1".parse().unwrap(),
                initial_margin: "0".parse().unwrap(),
                maintenance_margin: "0".parse().unwrap(),
                min_initial_margin: "0".parse().unwrap(),
                min_maintenance_margin: "0".parse().unwrap(),
                price_scale: 0,
                quantity_scale: 0,
                quote_asset: "USDT".to_string(),
                contract_type: "CRYPTO_OPTIONS".to_string(),
                underlying_type: "CRYPTO".to_string(),
                naked_sell: None,
                status: "TRADING".to_string(),
                filters: Vec::new(),
            },
        );

        assert!(service.sync_instrument().await.is_err());
        assert_eq!(live_instruments.read().await.len(), 1);
    }
}

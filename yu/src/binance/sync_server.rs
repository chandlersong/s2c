use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_funding_rate_table, get_swap_kline_table};
use crate::binance::bn_dashboard::{BinanceDashboard, InstrumentProvider, SpotInstrumentProvider, SwapInstrumentProvider};
use crate::binance::db_consts::BinanceTables;
use crate::binance::duckdb_repository::{
    BNInstrumentRepository, BinanceFundingRateRepository, BinanceKlineRepository, BinanceTradeRepository, get_default_funding_rate_repo,
    get_default_spot_kline_repo, get_default_spot_trade_repo, get_default_swap_kline_repo, get_instrument_repo_for_production,
};
use crate::binance::history::initial_kline;
use crate::binance::models::po::{BinanceInstrument, FundingRatePo, KlinePo, SpotStreamTradeRecordPo};
use crate::data_integrity::check::{BinarySearchDS, DuckDBBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, ServerMessage};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc;
use log::{debug, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::Status;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandlerTrait, HistoryFetcherImpl, HistoryFetcherTrait, ShareHistoryBatchHandler};
use yue::errors::YueError as ExchangeError;
use yue::models::HistoryInterval;
use yue::query_message::{BatchInsertPayload, QueryCommand};

#[cfg(test)]
mod tests {
    use super::*;
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

pub struct BinanceSpotTradeSyncHistoryService {
    trade_repo: BinanceTradeRepository,
}

impl BinanceSpotTradeSyncHistoryService {
    pub fn for_production() -> Arc<Self> {
        Arc::new(Self {
            trade_repo: get_default_spot_trade_repo(None),
        })
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for BinanceSpotTradeSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let trades = self.trade_repo.find_by_inst_id_between(inst_id, start_ts, end_ts).await?;
        if trades.is_empty() {
            let message = ServerMessage {
                history: vec![],
                server_timestamp: unix_time_now_u64_utc(),
            };
            let _ = tx.send(Ok(message)).await;
            return Ok(());
        }

        for batch in trades.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(SpotStreamTradeRecordPo::to_spot_history_entry).collect(),
                server_timestamp: unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        debug!("Binance Spot trade history check is not implemented; trade records are live-stream data");
        Ok(())
    }

    async fn initial_history_data(&self, _end_ms: Option<u64>) -> Result<(), YuError> {
        debug!("Binance Spot trade history initialization is not implemented; no historical backfill is configured");
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        debug!("Binance Spot trade sync is driven by WebSocket");
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

struct BinanceGapKlineSaver {
    db: crate::duck_db_tables::DuckTableTableChannel<KlinePo>,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
}

impl BinanceGapKlineSaver {
    fn new(
        db: crate::duck_db_tables::DuckTableTableChannel<KlinePo>,
        history_tx: mpsc::Sender<HistoryEntry>,
        inst_id: u64,
    ) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { db, history_tx, inst_id }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for BinanceGapKlineSaver {
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), ExchangeError> {
        let klines = batch_data
            .into_iter()
            .map(|kline| KlinePo::from_binance_kline(kline, self.inst_id))
            .collect::<Vec<_>>();
        self.db
            .send(QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(klines.clone())))
            .await
            .map_err(|error| ExchangeError::CustomError(format!("failed to enqueue Binance gap Kline batch: {error}")))?;
        for kline in klines {
            self.history_tx
                .send(kline.to_spot_history_entry())
                .await
                .map_err(|error| ExchangeError::CustomError(format!("failed to broadcast Binance gap Kline: {error}")))?;
        }
        Ok(())
    }
}

pub struct BinanceSpotSyncHistoryService {
    data_retention_ms: u64,
    dashboard: Arc<BinanceDashboard>,
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
            dashboard,
            kline_repo: get_default_spot_kline_repo(None),
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
                server_timestamp: li::tools::time::unix_time_now_u64_utc(),
            };
            let _ = tx.send(Ok(message)).await;
            return Ok(());
        }

        for batch in klines.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(|kline| kline.to_spot_history_entry()).collect(),
                server_timestamp: li::tools::time::unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        let interval = self.interval.clone();
        let now_ms = interval.get_now_close_unix_ms_utc();
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);

        for instrument in self.dashboard.spot_living_symbols() {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms >= now_ms {
                continue;
            }

            let gaps = binary_search_gap(
                instrument.id.to_string().as_str(),
                "SPOT",
                start_ms,
                now_ms,
                &interval,
                self.binary_search_ds.clone(),
            )?;
            debug!("{} has {} gaps", instrument.symbol, gaps.len());

            for gap in gaps {
                match gap {
                    ValidationGap::MissingData { start_time, end_time, .. } => {
                        let fetcher = HistoryFetcherImpl::kline(&SPOT_KLINE_HISTORY_COMMAND);
                        let request = CommonRequestBuilder::new(instrument.symbol.clone(), 1000, interval.clone());
                        let saver = BinanceGapKlineSaver::new(get_spot_kline_table(), self.history_tx.clone(), instrument.id as u64);
                        fetcher
                            .get_all_kline_data(request, Some(interval.clone()), Some(start_time), Some(end_time), Some(saver), true)
                            .await?;
                    }
                    other => debug!("skip unsupported Binance history gap: {:?}", other),
                }
            }
        }
        Ok(())
    }

    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let instrument_provider = SpotInstrumentProvider::new_provider(self.dashboard.clone());

        initial_kline(
            instrument_provider,
            self.data_retention_ms,
            self.interval.clone(),
            get_spot_kline_table(),
            end_ms,
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
    db: crate::duck_db_tables::DuckTableTableChannel<KlinePo>,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
}

impl BinanceSwapGapKlineSaver {
    fn new(
        db: crate::duck_db_tables::DuckTableTableChannel<KlinePo>,
        history_tx: mpsc::Sender<HistoryEntry>,
        inst_id: u64,
    ) -> ShareHistoryBatchHandler<BinanceKline> {
        Arc::new(Self { db, history_tx, inst_id }) as Arc<dyn HistoryBatchHandlerTrait<BinanceKline> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<BinanceKline> for BinanceSwapGapKlineSaver {
    async fn handle(&self, batch_data: Vec<BinanceKline>) -> Result<(), ExchangeError> {
        let klines = batch_data
            .into_iter()
            .map(|kline| KlinePo::from_binance_kline(kline, self.inst_id))
            .collect::<Vec<_>>();
        self.db
            .send(QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(klines.clone())))
            .await
            .map_err(|error| ExchangeError::CustomError(format!("failed to enqueue Binance Swap gap Kline batch: {error}")))?;
        for kline in klines {
            self.history_tx
                .send(kline.to_swap_history_entry())
                .await
                .map_err(|error| ExchangeError::CustomError(format!("failed to broadcast Binance Swap gap Kline: {error}")))?;
        }
        Ok(())
    }
}

pub struct BinanceSwapSyncHistoryService {
    data_retention_ms: u64,
    dashboard: Arc<BinanceDashboard>,
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
            dashboard,
            kline_repo: get_default_swap_kline_repo(None),
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
                server_timestamp: li::tools::time::unix_time_now_u64_utc(),
            };
            let _ = tx.send(Ok(message)).await;
            return Ok(());
        }

        for batch in klines.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(|kline| kline.to_swap_history_entry()).collect(),
                server_timestamp: li::tools::time::unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        let interval = self.interval.clone();
        let now_ms = interval.get_now_close_unix_ms_utc();
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);

        for instrument in self.dashboard.swap_living_symbols() {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms >= now_ms {
                continue;
            }

            let gaps = binary_search_gap(
                instrument.id.to_string().as_str(),
                "SWAP",
                start_ms,
                now_ms,
                &interval,
                self.binary_search_ds.clone(),
            )?;
            debug!("{} has {} gaps", instrument.symbol, gaps.len());

            for gap in gaps {
                match gap {
                    ValidationGap::MissingData { start_time, end_time, .. } => {
                        let fetcher = HistoryFetcherImpl::kline(&SWAP_KLINE_HISTORY_COMMAND);
                        let request = CommonRequestBuilder::new(instrument.symbol.clone(), 1000, interval.clone());
                        let saver = BinanceSwapGapKlineSaver::new(get_swap_kline_table(), self.history_tx.clone(), instrument.id as u64);
                        fetcher
                            .get_all_kline_data(request, Some(interval.clone()), Some(start_time), Some(end_time), Some(saver), true)
                            .await?;
                    }
                    other => debug!("skip unsupported Binance Swap history gap: {:?}", other),
                }
            }
        }
        Ok(())
    }

    async fn initial_history_data(&self, start_initial_ms: Option<u64>) -> Result<(), YuError> {
        let instrument_provider = SwapInstrumentProvider::new_provider(self.dashboard.clone());
        initial_kline(
            instrument_provider,
            self.data_retention_ms,
            self.interval.clone(),
            get_swap_kline_table(),
            start_initial_ms,
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
            funding_rate_repo: get_default_funding_rate_repo(None),
            inst_provider: SwapInstrumentProvider::new_provider(dash_board),
            binary_search_ds,
            data_retention_ms,
            history_tx,
        })
    }

    async fn sync_funding_rate(&self, inst_id: u64, start_ms: u64, end_ms: u64, interval: HistoryInterval) -> Result<(), YuError> {
        let instrument = self
            .inst_provider
            .instruments(None)
            .into_iter()
            .find(|instrument| u64::try_from(instrument.id).ok() == Some(inst_id))
            .ok_or_else(|| YuError::new(&format!("Binance Swap instrument {inst_id} is unavailable for Funding Rate sync")))?;
        let request = CommonRequestBuilder::new(instrument.symbol, 1000, interval.clone());
        let saver = BinanceSwapFundingRateGapSaver::new(get_swap_funding_rate_table(), self.history_tx.clone(), inst_id);
        HistoryFetcherImpl::swap_funding_rate()
            .get_all_kline_data(request, Some(interval), Some(start_ms), Some(end_ms), Some(saver), true)
            .await?;
        Ok(())
    }
}

struct BinanceSwapFundingRateGapSaver {
    db: crate::duck_db_tables::DuckTableTableChannel<FundingRatePo>,
    history_tx: mpsc::Sender<HistoryEntry>,
    inst_id: u64,
}

impl BinanceSwapFundingRateGapSaver {
    fn new(
        db: crate::duck_db_tables::DuckTableTableChannel<FundingRatePo>,
        history_tx: mpsc::Sender<HistoryEntry>,
        inst_id: u64,
    ) -> ShareHistoryBatchHandler<FundingRate> {
        Arc::new(Self { db, history_tx, inst_id }) as Arc<dyn HistoryBatchHandlerTrait<FundingRate> + Send>
    }
}

#[async_trait]
impl HistoryBatchHandlerTrait<FundingRate> for BinanceSwapFundingRateGapSaver {
    async fn handle(&self, batch_data: Vec<FundingRate>) -> Result<(), ExchangeError> {
        let funding_rates = batch_data
            .into_iter()
            .map(|rate| FundingRatePo::from_funding_rate(rate, self.inst_id))
            .collect::<Vec<_>>();
        self.db
            .send(QueryCommand::BatchInsert(BatchInsertPayload::new_no_replay(funding_rates.clone())))
            .await
            .map_err(|error| ExchangeError::CustomError(format!("failed to enqueue Binance Swap Funding Rate batch: {error}")))?;
        for funding_rate in funding_rates {
            self.history_tx
                .send(funding_rate.to_swap_history_entry())
                .await
                .map_err(|error| ExchangeError::CustomError(format!("failed to broadcast Binance Swap Funding Rate: {error}")))?;
        }
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
                    server_timestamp: li::tools::time::unix_time_now_u64_utc(),
                }))
                .await;
            return Ok(());
        }

        for batch in funding_rates.chunks(batch_size.max(1)) {
            let message = ServerMessage {
                history: batch.iter().map(FundingRatePo::to_swap_history_entry).collect(),
                server_timestamp: li::tools::time::unix_time_now_u64_utc(),
            };
            if tx.send(Ok(message)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let now_ms = unix_time_now_u64_utc();
        let retention_floor = now_ms.saturating_sub(self.data_retention_ms);
        let now_ms = interval.get_now_close_unix_ms_utc();
        let trading_instruments = self.inst_provider.instruments(None);
        for instrument in trading_instruments {
            let instrument_start = instrument.on_board_time.unwrap_or(retention_floor).max(retention_floor);
            let start_ms = interval.get_close_unix_ms(instrument_start).saturating_add(interval.to_milliseconds());
            if start_ms >= now_ms {
                continue;
            }

            let gaps = binary_search_gap(
                instrument.id.to_string().as_str(),
                "SWAP",
                start_ms,
                now_ms,
                &interval,
                self.binary_search_ds.clone(),
            )?;
            debug!("{} has {} gaps", instrument.symbol, gaps.len());

            for gap in gaps {
                match gap {
                    ValidationGap::MissingData { start_time, end_time, .. } => {
                        self.sync_funding_rate(instrument.id as u64, start_time, end_time, HistoryInterval::OneHour)
                            .await?;
                    }
                    other => debug!("skip unsupported Binance Swap history gap: {:?}", other),
                }
            }
        }

        Ok(())
    }

    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let interval = HistoryInterval::OneHour;
        let start = interval.get_close_unix_ms(unix_time_now_u64_utc() - self.data_retention_ms);
        let instruments = self.inst_provider.instruments(None);
        let total = instruments.len();
        let end = end_ms.unwrap_or_else(|| unix_time_now_u64_utc());
        let mut completed = 0;
        let mut progress_interval = tokio::time::interval_at(tokio::time::Instant::now() + Duration::from_mins(1), Duration::from_mins(5));
        for instrument in instruments {
            let sync = self.sync_funding_rate(instrument.id as u64, start, end, interval.clone());
            tokio::pin!(sync);
            loop {
                tokio::select! {
                    result = &mut sync => {
                        result?;
                        completed += 1;
                        break;
                    }
                    _ = progress_interval.tick() => {
                        info!(
                            "binance Swap Funding Rate history initialization progress: {}/{} instruments completed",
                            completed,
                            total
                        );
                    }
                }
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
        let start = interval.get_now_close_unix_ms_utc() - interval.to_milliseconds() + 1;
        let end = interval.get_now_close_unix_ms_utc() + 30 * 1000;
        let instruments = self.inst_provider.instruments(None);
        for instrument in instruments {
            self.sync_funding_rate(instrument.id as u64, start, end, interval.clone()).await?;
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
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
}

impl BinanceSyncInstrumentService {
    pub fn for_production(dashboard: Arc<BinanceDashboard>, support_history_service: HashMap<HistoryType, SyncHistoryService>) -> Arc<Self> {
        Arc::new(Self {
            dashboard,
            instrument_repo: get_instrument_repo_for_production(None),
            support_history_service,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for BinanceSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        let mut instruments = self
            .instrument_repo
            .get_trading_instrument_by_type(yue::models::InstrumentType::Spot)
            .await?
            .into_iter()
            .map(BinanceInstrument::into_proto_instrument)
            .collect::<Vec<_>>();
        instruments.extend(
            self.instrument_repo
                .get_trading_instrument_by_type(yue::models::InstrumentType::Swap)
                .await?
                .into_iter()
                .map(BinanceInstrument::into_proto_instrument),
        );
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
        Ok(instruments)
    }

    fn error_log(&self) -> String {
        "error when syncing Binance instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<HistoryType, SyncHistoryService>, YuError> {
        Ok(self.support_history_service.clone())
    }
}

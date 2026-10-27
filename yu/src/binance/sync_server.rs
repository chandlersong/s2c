use crate::binance::bn_backend_service::{get_spot_kline_table, get_swap_kline_table};
use crate::binance::bn_dashboard::{BinanceDashboard, SpotInstrumentProvider, SwapInstrumentProvider};
use crate::binance::db_consts::BinanceTables;
use crate::binance::duckdb_repository::{
    BNInstrumentRepository, BinanceKlineRepository, get_default_spot_kline_repo, get_default_swap_kline_repo, get_instrument_repo_for_production,
};
use crate::binance::history::initial_kline;
use crate::binance::models::po::{BinanceInstrument, KlinePo};
use crate::data_integrity::check::{DuckDBBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, ServerMessage};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::Status;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::restful_func::{CommonRequestBuilder, HistoryBatchHandlerTrait, HistoryFetcherImpl, HistoryFetcherTrait, ShareHistoryBatchHandler};
use yue::errors::YueError as ExchangeError;
use yue::models::HistoryInterval;
use yue::query_message::{BatchInsertPayload, QueryCommand};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::models::grpc_sync::{InstrumentType, history_entry};

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
        Arc::new(Self {
            data_retention_ms,
            dashboard,
            kline_repo: get_default_spot_kline_repo(None),
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
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::SpotKline.table_name(),
            "candle_begin_time".to_string(),
            "inst_id".to_string(),
        );

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
                binary_search_ds.clone(),
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

    async fn initial_history_data(&self) -> Result<(), YuError> {
        let instrument_provider = SpotInstrumentProvider::new_provider(self.dashboard.clone());

        initial_kline(instrument_provider, self.data_retention_ms, self.interval.clone(), get_spot_kline_table()).await
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
        Arc::new(Self {
            data_retention_ms,
            dashboard,
            kline_repo: get_default_swap_kline_repo(None),
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
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            DuckDBDSProvider::default(),
            BinanceTables::SwapKline.table_name(),
            "candle_begin_time".to_string(),
            "inst_id".to_string(),
        );

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
                binary_search_ds.clone(),
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

    async fn initial_history_data(&self) -> Result<(), YuError> {
        let instrument_provider = SwapInstrumentProvider::new_provider(self.dashboard.clone());
        initial_kline(instrument_provider, self.data_retention_ms, self.interval.clone(), get_swap_kline_table()).await
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        debug!("Binance Swap latest kline sync is driven by WebSocket");
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

pub struct BinanceSyncSpotInstrumentService {
    dashboard: Arc<BinanceDashboard>,
    instrument_repo: BNInstrumentRepository,
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
}

impl BinanceSyncSpotInstrumentService {
    pub fn for_production(dashboard: Arc<BinanceDashboard>, support_history_service: HashMap<HistoryType, SyncHistoryService>) -> Arc<Self> {
        Arc::new(Self {
            dashboard,
            instrument_repo: get_instrument_repo_for_production(None),
            support_history_service,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for BinanceSyncSpotInstrumentService {
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

use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::{get_binance_kline_batch_insert, get_binance_swap_kline_batch_insert};
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::binance::{LocalBinanceInstrumentPo, LocalBinanceKlinePo};
use crate::sync::client::repository::binance::{ClientBinanceRepository, ClientBinanceRepositoryImpl};
use crate::sync::client::sync_client_service::{
    ClientHistoryService, ClientHistoryServiceTrait, ClientInstrumentService, ClientInstrumentServiceTrait,
};
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, InstrumentType, SyncRequest, history_entry::Payload, instrument};
use async_trait::async_trait;
use futures::StreamExt;
use futures::executor::block_on;
use li::tools::time::UnixTimeStamp;
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use yue::models::HistoryInterval;

type ServerIdToLocalId = Arc<RwLock<HashMap<u64, u64>>>;

pub struct BinanceSpotClientInstrumentService {
    history_services: HashMap<HistoryType, ClientHistoryService>,
}

impl BinanceSpotClientInstrumentService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        let repository = ClientBinanceRepositoryImpl::from_pool(pool.clone());
        let id_maps = block_on(repository.list_all_instruments())?
            .into_iter()
            .map(|instrument| (instrument.server_id, instrument.id))
            .collect();
        let id_maps = Arc::new(RwLock::new(id_maps));
        let kline_service = BinanceSpotKlineClientHistoryService::for_production(repository.clone(), pool.clone(), id_maps.clone());
        let swap_kline_service = BinanceSwapKlineClientHistoryService::for_production(repository, pool, id_maps);
        Ok(Arc::new(Self {
            history_services: HashMap::from([(HistoryType::BinanceSpot, kline_service), (HistoryType::BinanceSwap, swap_kline_service)]),
        }))
    }
}

#[async_trait]
impl ClientInstrumentServiceTrait for BinanceSpotClientInstrumentService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload.as_ref() else {
            return Err(YuError::new("instrument payload is not a Binance instrument"));
        };
        let history_type = match binance_instrument.symbol_type.to_ascii_uppercase().as_str() {
            "SPOT" => HistoryType::BinanceSpot,
            "SWAP" | "PERPETUAL" => HistoryType::BinanceSwap,
            _ => return Err(YuError::new("unsupported Binance instrument symbol type")),
        };
        self.history_services
            .get(&history_type)
            .ok_or_else(|| YuError::new("Binance history service is not configured"))?
            .initial_data(instrument, end_ms)
            .await
    }

    fn supported_history_services(&self) -> Result<HashMap<HistoryType, ClientHistoryService>, YuError> {
        Ok(self.history_services.clone())
    }
}

pub struct BinanceSwapKlineClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceKlinePo>,
    repository: ClientBinanceRepository,
    interval: HistoryInterval,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    id_maps: ServerIdToLocalId,
}

impl BinanceSwapKlineClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, id_maps: ServerIdToLocalId) -> ClientHistoryService {
        let binary_search = SyncClientBinarySearchDataImpl::new_with_instrument_column(
            pool,
            ClientsTables::BinanceSwapKline.table_name(),
            ClientsTables::BinanceInstruments.table_name(),
            "inst_id",
            "candle_begin_time",
        );
        Arc::new(Self {
            history_batch_insert: get_binance_swap_kline_batch_insert(),
            repository,
            interval: HistoryInterval::OneMinute,
            grpc_manager: get_grpc_manager_from_config(),
            binary_search,
            id_maps,
        })
    }

    fn first_candle_timestamp(&self, on_board_time: u64) -> u64 {
        let aligned_time = self.interval.get_close_unix_ms(on_board_time);
        if aligned_time < on_board_time {
            aligned_time.saturating_add(self.interval.to_milliseconds())
        } else {
            aligned_time
        }
    }

    async fn backfill_history(&self, server_inst_id: u64, local_inst_id: u64, start_ms: u64, end_ms: u64) -> Result<(), YuError> {
        if start_ms > end_ms {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: server_inst_id,
                start_ms,
                end_ms,
                history_type: HistoryType::BinanceSwap as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            for entry in message?.history {
                if entry.history_type != HistoryType::BinanceSwap as i32
                    || entry.instrument_type != InstrumentType::Binance as i32
                    || entry.inst_id != server_inst_id
                    || !matches!(entry.payload.as_ref(), Some(Payload::BinanceKline(_)))
                {
                    return Err(YuError::new("received invalid Binance swap kline history entry"));
                }
                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceSwapKlineClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Binance as i32 {
            return Err(YuError::new("instrument exchange is not Binance"));
        }
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not Binance"));
        };
        if !matches!(binance_instrument.symbol_type.to_ascii_uppercase().as_str(), "SWAP" | "PERPETUAL") {
            return Err(YuError::new("instrument is not a Binance swap instrument"));
        }

        let cached_local_inst_id = self.id_maps.read().await.get(&binance_instrument.server_id).copied();
        let local_inst_id = match cached_local_inst_id {
            Some(local_inst_id) => local_inst_id,
            None => {
                let local_inst_id = match self.repository.find_instrument_id_by_server_id(binance_instrument.server_id).await? {
                    Some(local_inst_id) => local_inst_id,
                    None => {
                        self.repository
                            .create_instrument(LocalBinanceInstrumentPo::from_proto(binance_instrument.clone()))
                            .await?
                    }
                };
                self.id_maps.write().await.insert(binance_instrument.server_id, local_inst_id);
                local_inst_id
            }
        };

        let start_ms = match self
            .repository
            .find_latest_swap_kline_timestamp_by_server_id(binance_instrument.server_id)
            .await?
        {
            Some(timestamp) => timestamp.saturating_add(1),
            None => {
                let Some(on_board_time) = binance_instrument.on_board_time else {
                    return Err(YuError::new("Binance swap instrument listing time is unavailable"));
                };
                self.first_candle_timestamp(on_board_time)
            }
        };
        let end_ms = self.interval.get_close_unix_ms(end_ms);
        self.backfill_history(binance_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        for instrument in self.repository.list_all_instruments().await? {
            if !matches!(instrument.symbol_type.to_ascii_uppercase().as_str(), "SWAP" | "PERPETUAL")
                || !instrument.status.eq_ignore_ascii_case("TRADING")
            {
                continue;
            }
            let Some(on_board_time) = instrument.on_board_time else {
                warn!(
                    "Binance swap instrument {} has no listing time; skipping history reconciliation",
                    instrument.server_id
                );
                continue;
            };
            let start_timestamp = self.first_candle_timestamp(on_board_time);
            let gaps = binary_search_gap(
                &instrument.server_id.to_string(),
                "SWAP",
                start_timestamp,
                end_timestamp,
                &self.interval,
                self.binary_search.clone(),
            )?;
            for gap in gaps {
                if let ValidationGap::MissingData { start_time, end_time, .. } = gap {
                    self.backfill_history(instrument.server_id, instrument.id, start_time, end_time.saturating_sub(1))
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::BinanceSwap as i32
            || history_entry.instrument_type != InstrumentType::Binance as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::BinanceKline(_)))
        {
            return Err(YuError::new("received non-Binance swap kline history payload"));
        }

        let local_inst_id = match self.id_maps.read().await.get(&history_entry.inst_id).copied() {
            Some(local_inst_id) => local_inst_id,
            None => {
                let Some(local_inst_id) = self.repository.find_instrument_id_by_server_id(history_entry.inst_id).await? else {
                    warn!("local Binance instrument {} not found; skipping history", history_entry.inst_id);
                    return Ok(());
                };
                self.id_maps.write().await.insert(history_entry.inst_id, local_inst_id);
                local_inst_id
            }
        };
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
}

pub struct BinanceSpotKlineClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceKlinePo>,
    repository: ClientBinanceRepository,
    interval: HistoryInterval,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    id_maps: ServerIdToLocalId,
}

impl BinanceSpotKlineClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, id_maps: ServerIdToLocalId) -> ClientHistoryService {
        let binary_search = SyncClientBinarySearchDataImpl::new_with_instrument_column(
            pool,
            ClientsTables::BinanceSpotKline.table_name(),
            ClientsTables::BinanceInstruments.table_name(),
            "inst_id",
            "candle_begin_time",
        );
        Arc::new(Self {
            history_batch_insert: get_binance_kline_batch_insert(),
            repository,
            interval: HistoryInterval::OneMinute,
            grpc_manager: get_grpc_manager_from_config(),
            binary_search,
            id_maps,
        })
    }

    fn first_candle_timestamp(&self, on_board_time: u64) -> u64 {
        let aligned_time = self.interval.get_close_unix_ms(on_board_time);
        if aligned_time < on_board_time {
            aligned_time.saturating_add(self.interval.to_milliseconds())
        } else {
            aligned_time
        }
    }

    async fn backfill_history(&self, server_inst_id: u64, local_inst_id: u64, start_ms: u64, end_ms: u64) -> Result<(), YuError> {
        if start_ms > end_ms {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: server_inst_id,
                start_ms,
                end_ms,
                history_type: HistoryType::BinanceSpot as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            for entry in message?.history {
                if entry.history_type != HistoryType::BinanceSpot as i32
                    || entry.instrument_type != InstrumentType::Binance as i32
                    || entry.inst_id != server_inst_id
                    || !matches!(entry.payload.as_ref(), Some(Payload::BinanceKline(_)))
                {
                    return Err(YuError::new("received invalid Binance spot kline history entry"));
                }
                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceSpotKlineClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Binance as i32 {
            return Err(YuError::new("instrument exchange is not Binance"));
        }
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not Binance"));
        };
        if !binance_instrument.symbol_type.eq_ignore_ascii_case("SPOT") {
            return Err(YuError::new("instrument is not a Binance spot instrument"));
        }

        let cached_local_inst_id = self.id_maps.read().await.get(&binance_instrument.server_id).copied();
        let local_inst_id = match cached_local_inst_id {
            Some(local_inst_id) => local_inst_id,
            None => {
                let local_inst_id = match self.repository.find_instrument_id_by_server_id(binance_instrument.server_id).await? {
                    Some(local_inst_id) => local_inst_id,
                    None => {
                        self.repository
                            .create_instrument(LocalBinanceInstrumentPo::from_proto(binance_instrument.clone()))
                            .await?
                    }
                };
                self.id_maps.write().await.insert(binance_instrument.server_id, local_inst_id);
                local_inst_id
            }
        };

        let start_ms = match self
            .repository
            .find_latest_kline_timestamp_by_server_id(binance_instrument.server_id)
            .await?
        {
            Some(timestamp) => timestamp.saturating_add(1),
            None => {
                let Some(on_board_time) = binance_instrument.on_board_time else {
                    return Err(YuError::new("Binance spot instrument listing time is unavailable"));
                };
                self.first_candle_timestamp(on_board_time)
            }
        };
        let end_ms = self.interval.get_close_unix_ms(end_ms);
        self.backfill_history(binance_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        for instrument in self.repository.list_all_instruments().await? {
            if !instrument.symbol_type.eq_ignore_ascii_case("SPOT") || !instrument.status.eq_ignore_ascii_case("TRADING") {
                continue;
            }
            let Some(on_board_time) = instrument.on_board_time else {
                warn!(
                    "Binance spot instrument {} has no listing time; skipping history reconciliation",
                    instrument.server_id
                );
                continue;
            };
            let start_timestamp = self.first_candle_timestamp(on_board_time);
            let gaps = binary_search_gap(
                &instrument.server_id.to_string(),
                "SPOT",
                start_timestamp,
                end_timestamp,
                &self.interval,
                self.binary_search.clone(),
            )?;
            for gap in gaps {
                if let ValidationGap::MissingData { start_time, end_time, .. } = gap {
                    self.backfill_history(instrument.server_id, instrument.id, start_time, end_time.saturating_sub(1))
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::BinanceSpot as i32
            || history_entry.instrument_type != InstrumentType::Binance as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::BinanceKline(_)))
        {
            return Err(YuError::new("received non-Binance spot kline history payload"));
        }

        let local_inst_id = match self.id_maps.read().await.get(&history_entry.inst_id).copied() {
            Some(local_inst_id) => local_inst_id,
            None => {
                let Some(local_inst_id) = self.repository.find_instrument_id_by_server_id(history_entry.inst_id).await? else {
                    warn!("local Binance instrument {} not found; skipping history", history_entry.inst_id);
                    return Ok(());
                };
                self.id_maps.write().await.insert(history_entry.inst_id, local_inst_id);
                local_inst_id
            }
        };
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::check::MockBinarySearchDSTrait;
    use crate::postgresql_db_tables::MockPostgresqlBatchInsertTrait;
    use crate::sync::client::grpc_manager::MockGrpcChannelManagerTrait;
    use crate::sync::client::repository::binance::MockClientBinanceRepositoryTrait;
    use crate::sync::models::grpc_sync::{BinanceKline, HistoryType, InstrumentType};

    fn mock_binary_search() -> BinarySearchDS {
        Arc::new(MockBinarySearchDSTrait::new())
    }

    // 测试目的：验证实时 Binance Spot Kline 根据服务端 instrument ID 映射到本地 ID 后提交批量写入。
    // 准备数据/过程：缓存 server ID 42 到 local ID 7，并提交一条合法的 Binance Spot Kline history entry。
    // 预期结果：batch insert 被调用一次，传入的本地 instrument ID 为 7。
    #[tokio::test]
    async fn insert_history_uses_cached_local_instrument_id() {
        let repository = MockClientBinanceRepositoryTrait::new();
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceKlinePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.inst_id == 42
                    && *local_inst_id == 7
                    && matches!(entry.payload.as_ref(), Some(Payload::BinanceKline(kline)) if kline.candle_begin_time == 60_000)
            })
            .times(1)
            .returning(|_, _| ());

        let service = BinanceSpotKlineClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            interval: HistoryInterval::OneMinute,
            grpc_manager: Arc::new(MockGrpcChannelManagerTrait::new()),
            binary_search: mock_binary_search(),
            id_maps: Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
        };
        let entry = HistoryEntry {
            history_type: HistoryType::BinanceSpot as i32,
            instrument_type: InstrumentType::Binance as i32,
            inst_id: 42,
            payload: Some(Payload::BinanceKline(BinanceKline {
                id: 1,
                candle_begin_time: 60_000,
                open: 1.0,
                high: 2.0,
                low: 0.5,
                close: 1.5,
                volume: 3.0,
                quote_volume: 4.0,
                number_of_trades: 5,
                taker_buy_base_asset_volume: 2.0,
                taker_buy_quote_asset_volume: 3.0,
                close_time: 119_999,
                interval: 1,
                first_trade_id: Some(10),
                last_trade_id: Some(14),
            })),
        };

        service.insert_history(entry).await.unwrap();
    }

    // 测试目的：验证实时 Binance Swap Kline 使用共享的服务端到本地 instrument ID 映射并写入 Swap 批量队列。
    // 准备数据/过程：缓存 server ID 42 到 local ID 7，提交一条合法的 BinanceSwap Kline history entry。
    // 预期结果：Swap batch insert 被调用一次，且保留服务端 history 类型并使用本地 instrument ID 7。
    #[tokio::test]
    async fn swap_insert_history_uses_cached_local_instrument_id() {
        let repository = MockClientBinanceRepositoryTrait::new();
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceKlinePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.history_type == HistoryType::BinanceSwap as i32
                    && entry.inst_id == 42
                    && *local_inst_id == 7
                    && matches!(entry.payload.as_ref(), Some(Payload::BinanceKline(kline)) if kline.candle_begin_time == 60_000)
            })
            .times(1)
            .returning(|_, _| ());

        let service = BinanceSwapKlineClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            interval: HistoryInterval::OneMinute,
            grpc_manager: Arc::new(MockGrpcChannelManagerTrait::new()),
            binary_search: mock_binary_search(),
            id_maps: Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
        };
        let entry = HistoryEntry {
            history_type: HistoryType::BinanceSwap as i32,
            instrument_type: InstrumentType::Binance as i32,
            inst_id: 42,
            payload: Some(Payload::BinanceKline(BinanceKline {
                id: 1,
                candle_begin_time: 60_000,
                open: 1.0,
                high: 2.0,
                low: 0.5,
                close: 1.5,
                volume: 3.0,
                quote_volume: 4.0,
                number_of_trades: 5,
                taker_buy_base_asset_volume: 2.0,
                taker_buy_quote_asset_volume: 3.0,
                close_time: 119_999,
                interval: 1,
                first_trade_id: Some(10),
                last_trade_id: Some(14),
            })),
        };

        service.insert_history(entry).await.unwrap();
    }

    // 测试目的：验证 Binance PERPETUAL instrument 被路由到 Swap history service，而不误调 Spot service。
    // 准备数据/过程：仅注册 Swap mock service，并向 instrument service 提交 symbol_type 为 PERPETUAL 的 Binance instrument。
    // 预期结果：Swap service 收到一次该 instrument，调用成功。
    #[tokio::test]
    async fn initial_data_routes_perpetual_instrument_to_swap_service() {
        let mut swap_history_service = crate::sync::client::sync_client_service::MockClientHistoryServiceTrait::new();
        swap_history_service
            .expect_initial_data()
            .withf(|instrument, _| {
                matches!(
                    instrument.payload.as_ref(),
                    Some(instrument::Payload::Binance(binance_instrument)) if binance_instrument.symbol_type == "PERPETUAL"
                )
            })
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        let swap_history_service: ClientHistoryService = Arc::new(swap_history_service);
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::from([(HistoryType::BinanceSwap, swap_history_service)]),
        };
        let instrument = Instrument {
            instrument_type: InstrumentType::Binance as i32,
            payload: Some(instrument::Payload::Binance(crate::sync::models::grpc_sync::BinanceInstrument {
                server_id: 42,
                symbol: "BTCUSDT".to_string(),
                status: "TRADING".to_string(),
                base_asset: "BTC".to_string(),
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 8,
                order_types: vec!["LIMIT".to_string()],
                symbol_type: "PERPETUAL".to_string(),
                on_board_time: Some(1_700_000_000_000),
            })),
        };

        service.initial_data(instrument, 1_700_000_060_000).await.unwrap();
    }
}

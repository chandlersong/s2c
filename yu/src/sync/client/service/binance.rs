use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::{
    get_binance_kline_batch_insert, get_binance_swap_funding_rate_batch_insert, get_binance_swap_kline_batch_insert,
};
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::binance::{BinanceInstrumentType, LocalBinanceFundingRatePo, LocalBinanceInstrumentPo, LocalBinanceKlinePo};
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
use yue::models::{DEFAULT_START_TIMESTAMP, HistoryInterval};

type ServerIdToLocalId = Arc<RwLock<HashMap<u64, u64>>>;

pub struct BinanceSpotClientInstrumentService {
    history_services: HashMap<HistoryType, ClientHistoryService>,
}

impl BinanceSpotClientInstrumentService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        let repository = ClientBinanceRepositoryImpl::from_pool(pool.clone());
        let spot_instruments = block_on(repository.list_all_instruments(BinanceInstrumentType::Spot))?;
        let swap_instruments = block_on(repository.list_all_instruments(BinanceInstrumentType::Swap))?;
        let id_maps = spot_instruments
            .into_iter()
            .chain(swap_instruments)
            .map(|instrument| (instrument.server_id, instrument.id))
            .collect();
        let id_maps = Arc::new(RwLock::new(id_maps));
        let kline_service = BinanceSpotKlineClientHistoryService::for_production(repository.clone(), pool.clone(), id_maps.clone());
        let swap_kline_service = BinanceSwapKlineClientHistoryService::for_production(repository.clone(), pool.clone(), id_maps.clone());
        let swap_funding_rate_service = BinanceSwapFundingRateClientHistoryService::for_production(repository, pool.clone(), id_maps);
        let swap_service: ClientHistoryService = Arc::new(BinanceSwapClientHistoryService {
            kline: swap_kline_service,
            funding_rate: swap_funding_rate_service.clone(),
        });
        Ok(Arc::new(Self {
            history_services: HashMap::from([
                (HistoryType::BinanceSpot, kline_service),
                (HistoryType::BinanceSwap, swap_service),
                (HistoryType::BinanceSwapFundingRate, swap_funding_rate_service),
            ]),
        }))
    }
}

#[async_trait]
impl ClientInstrumentServiceTrait for BinanceSpotClientInstrumentService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload.as_ref() else {
            return Err(YuError::new("instrument payload is not a Binance instrument"));
        };
        let history_type = match BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type) {
            Some(BinanceInstrumentType::Spot) => HistoryType::BinanceSpot,
            Some(BinanceInstrumentType::Swap) => HistoryType::BinanceSwap,
            None => return Err(YuError::new("unsupported Binance instrument symbol type")),
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
        if BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type) != Some(BinanceInstrumentType::Swap) {
            return Err(YuError::new("instrument is not a Binance swap instrument"));
        }

        let cached_local_inst_id = { self.id_maps.read().await.get(&binance_instrument.server_id).copied() };
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
        for instrument in self.repository.list_all_instruments(BinanceInstrumentType::Swap).await? {
            if !instrument.status.eq_ignore_ascii_case("TRADING") {
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

        let cached_local_inst_id = { self.id_maps.read().await.get(&history_entry.inst_id).copied() };
        let local_inst_id = match cached_local_inst_id {
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

pub struct BinanceSwapFundingRateClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceFundingRatePo>,
    repository: ClientBinanceRepository,
    interval: HistoryInterval,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    id_maps: ServerIdToLocalId,
}

struct BinanceSwapClientHistoryService {
    kline: ClientHistoryService,
    funding_rate: ClientHistoryService,
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceSwapClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        self.kline.initial_data(instrument.clone(), end_ms).await?;
        self.funding_rate.initial_data(instrument, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        self.kline.reconcile_living_instrument_with_server(end_ms).await
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type == HistoryType::BinanceSwapFundingRate as i32 {
            self.funding_rate.insert_history(history_entry).await
        } else {
            self.kline.insert_history(history_entry).await
        }
    }
}

impl BinanceSwapFundingRateClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, id_maps: ServerIdToLocalId) -> ClientHistoryService {
        let interval = HistoryInterval::OneHour;
        let binary_search = SyncClientBinarySearchDataImpl::new_with_instrument_column(
            pool,
            ClientsTables::BinanceSwapFundingRate.table_name(),
            ClientsTables::BinanceInstruments.table_name(),
            "inst_id",
            "funding_time",
        );
        Arc::new(Self {
            history_batch_insert: get_binance_swap_funding_rate_batch_insert(),
            repository,
            interval,
            grpc_manager: get_grpc_manager_from_config(),
            binary_search,
            id_maps,
        })
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
                history_type: HistoryType::BinanceSwapFundingRate as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            for entry in message?.history {
                if entry.history_type != HistoryType::BinanceSwapFundingRate as i32
                    || entry.instrument_type != InstrumentType::Binance as i32
                    || entry.inst_id != server_inst_id
                    || !matches!(entry.payload.as_ref(), Some(Payload::BinanceFundingRate(_)))
                {
                    return Err(YuError::new("received invalid Binance swap funding rate history entry"));
                }
                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceSwapFundingRateClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Binance as i32 {
            return Err(YuError::new("instrument exchange is not Binance"));
        }
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not Binance"));
        };
        if BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type) != Some(BinanceInstrumentType::Swap) {
            return Err(YuError::new("instrument is not a Binance swap instrument"));
        }

        let cached_local_inst_id = { self.id_maps.read().await.get(&binance_instrument.server_id).copied() };
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
            .find_latest_swap_funding_rate_timestamp_by_server_id(binance_instrument.server_id)
            .await?
        {
            Some(timestamp) => timestamp.saturating_add(1),
            None => binance_instrument
                .on_board_time
                .ok_or_else(|| YuError::new("Binance swap instrument listing time is unavailable"))?,
        };
        self.backfill_history(binance_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        for instrument in self.repository.list_all_instruments(BinanceInstrumentType::Swap).await? {
            if !instrument.status.eq_ignore_ascii_case("TRADING") {
                continue;
            }
            let Some(on_board_time) = instrument.on_board_time else {
                warn!(
                    "Binance swap instrument {} has no listing time; skipping funding rate reconciliation",
                    instrument.server_id
                );
                continue;
            };
            let start_timestamp = self
                .interval
                .get_close_unix_ms(on_board_time)
                .saturating_add(self.interval.to_milliseconds());
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
                } else {
                    warn!("unexpected Binance funding rate validation gap: {:?}", gap);
                }
            }
        }
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::BinanceSwapFundingRate as i32
            || history_entry.instrument_type != InstrumentType::Binance as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::BinanceFundingRate(_)))
        {
            return Err(YuError::new("received non-Binance swap funding rate history payload"));
        }

        let cached_local_inst_id = { self.id_maps.read().await.get(&history_entry.inst_id).copied() };
        let local_inst_id = match cached_local_inst_id {
            Some(local_inst_id) => local_inst_id,
            None => {
                let Some(local_inst_id) = self.repository.find_instrument_id_by_server_id(history_entry.inst_id).await? else {
                    warn!(
                        "local Binance instrument {} not found; skipping funding rate history",
                        history_entry.inst_id
                    );
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
        if BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type) != Some(BinanceInstrumentType::Spot) {
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
                let on_board_time = match binance_instrument.on_board_time {
                    Some(time) => time,
                    None => DEFAULT_START_TIMESTAMP,
                };
                self.first_candle_timestamp(on_board_time)
            }
        };
        let end_ms = self.interval.get_close_unix_ms(end_ms);
        self.backfill_history(binance_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        for instrument in self.repository.list_all_instruments(BinanceInstrumentType::Spot).await? {
            if !instrument.status.eq_ignore_ascii_case("TRADING") {
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
    use crate::sync::models::grpc_sync::{BinanceFundingRate, BinanceKline, HistoryType, InstrumentType};

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

    // 测试目的：验证 Funding Rate 历史消息通过 server instrument ID 查询并使用本地 instrument ID 入批。
    // 准备数据/过程：mock repository 将 server ID 42 映射为 local ID 7，并提交一条合法的 Funding Rate history entry。
    // 预期结果：批量写入仅调用一次，且消息类型、服务端 instrument ID 与本地 ID 映射正确。
    #[tokio::test]
    async fn funding_rate_insert_history_resolves_local_instrument_id() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository
            .expect_find_instrument_id_by_server_id()
            .with(mockall::predicate::eq(42))
            .times(1)
            .returning(|_| Ok(Some(7)));
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceFundingRatePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.history_type == HistoryType::BinanceSwapFundingRate as i32
                    && entry.inst_id == 42
                    && *local_inst_id == 7
                    && matches!(entry.payload.as_ref(), Some(Payload::BinanceFundingRate(rate)) if rate.funding_time == 1_700_000_000_123)
            })
            .times(1)
            .returning(|_, _| ());

        let service = BinanceSwapFundingRateClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            interval: HistoryInterval::OneHour,
            grpc_manager: Arc::new(MockGrpcChannelManagerTrait::new()),
            binary_search: mock_binary_search(),
            id_maps: Arc::new(RwLock::new(HashMap::new())),
        };
        let entry = HistoryEntry {
            history_type: HistoryType::BinanceSwapFundingRate as i32,
            instrument_type: InstrumentType::Binance as i32,
            inst_id: 42,
            payload: Some(Payload::BinanceFundingRate(BinanceFundingRate {
                id: 3,
                funding_time: 1_700_000_000_123,
                funding_rate: 0.0001,
                mark_price: None,
            })),
        };

        service.insert_history(entry).await.unwrap();
    }

    // 测试目的：验证 Funding Rate 对账通过二分查找缺口并将缺失区间回补到本地。
    // 准备数据/过程：mock 活跃 Perpetual instrument、缺口检测结果和服务端历史响应。
    // 预期结果：按 Funding Rate 类型查询 instrument，并以本地 instrument ID 批量写入缺口历史。
    #[tokio::test]
    async fn funding_rate_reconciliation_backfills_binary_search_gap() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository
            .expect_list_all_instruments()
            .with(mockall::predicate::eq(BinanceInstrumentType::Swap))
            .times(1)
            .returning(|_| {
                Ok(vec![LocalBinanceInstrumentPo {
                    id: 7,
                    server_id: 42,
                    symbol: "BTCUSDT".to_string(),
                    status: "TRADING".to_string(),
                    base_asset: "BTC".to_string(),
                    quote_asset: "USDT".to_string(),
                    quote_asset_precision: 8,
                    order_types: vec!["LIMIT".to_string()],
                    symbol_type: "SWAP".to_string(),
                    on_board_time: Some(1_000),
                }])
            });

        let interval = HistoryInterval::OneHour;
        let interval_ms = interval.to_milliseconds();
        let start_timestamp = interval_ms;
        let end_timestamp = interval_ms * 4;
        let mut binary_search = MockBinarySearchDSTrait::new();
        binary_search
            .expect_count_distinct_between()
            .withf(move |identify, start, end| identify == "42" && *start == start_timestamp && *end == end_timestamp)
            .times(1)
            .returning(|_, _, _| Ok(0));
        binary_search
            .expect_table_name()
            .times(1)
            .returning(|| "binance_swap_funding_rate_history".to_string());

        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(move |request| {
                request.inst_id == 42
                    && request.start_ms == interval_ms
                    && request.end_ms == interval_ms * 4 - 1
                    && request.history_type == HistoryType::BinanceSwapFundingRate as i32
            })
            .times(1)
            .returning(move |_| {
                Ok(Box::pin(futures::stream::iter(vec![Ok(crate::sync::models::grpc_sync::ServerMessage {
                    history: vec![HistoryEntry {
                        history_type: HistoryType::BinanceSwapFundingRate as i32,
                        instrument_type: InstrumentType::Binance as i32,
                        inst_id: 42,
                        payload: Some(Payload::BinanceFundingRate(BinanceFundingRate {
                            id: 2,
                            funding_time: interval_ms * 2,
                            funding_rate: 0.0002,
                            mark_price: Some(42_500.0),
                        })),
                    }],
                    server_timestamp: end_timestamp,
                })])) as crate::sync::client::grpc_manager::GrpcServerMessageStream)
            });
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceFundingRatePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(move |entry, local_inst_id| {
                *local_inst_id == 7
                    && matches!(entry.payload.as_ref(), Some(Payload::BinanceFundingRate(rate)) if rate.funding_time == interval_ms * 2)
            })
            .times(1)
            .returning(|_, _| ());

        let service = BinanceSwapFundingRateClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            interval,
            grpc_manager: Arc::new(grpc_manager),
            binary_search: Arc::new(binary_search),
            id_maps: Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
        };

        service.reconcile_living_instrument_with_server(end_timestamp).await.unwrap();
    }

    // 测试目的：验证 Binance Swap instrument 被路由到 Swap history service，而不误调 Spot service。
    // 准备数据/过程：仅注册 Swap mock service，并向 instrument service 提交 symbol_type 为 SWAP 的 Binance instrument。
    // 预期结果：Swap service 收到一次该 instrument，调用成功。
    #[tokio::test]
    async fn initial_data_routes_swap_instrument_to_swap_service() {
        let mut swap_history_service = crate::sync::client::sync_client_service::MockClientHistoryServiceTrait::new();
        swap_history_service
            .expect_initial_data()
            .withf(|instrument, _| {
                matches!(
                    instrument.payload.as_ref(),
                    Some(instrument::Payload::Binance(binance_instrument)) if binance_instrument.symbol_type == "SWAP"
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
                symbol_type: "SWAP".to_string(),
                on_board_time: Some(1_700_000_000_000),
            })),
        };

        service.initial_data(instrument, 1_700_000_060_000).await.unwrap();
    }

    // 测试目的：验证 Binance Swap instrument 初始化会同时启动 Kline 和 Funding Rate 历史回补。
    // 准备数据/过程：分别 mock 组合服务中的 Kline、Funding Rate 子服务，并提交 SWAP instrument。
    // 预期结果：两个子服务各收到一次相同 instrument 和结束时间，组合初始化成功。
    #[tokio::test]
    async fn swap_initial_data_backfills_kline_and_funding_rate() {
        let mut kline_service = crate::sync::client::sync_client_service::MockClientHistoryServiceTrait::new();
        kline_service
            .expect_initial_data()
            .withf(|instrument, end_ms| instrument.instrument_type == InstrumentType::Binance as i32 && *end_ms == 1_700_000_060_000)
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        let mut funding_rate_service = crate::sync::client::sync_client_service::MockClientHistoryServiceTrait::new();
        funding_rate_service
            .expect_initial_data()
            .withf(|instrument, end_ms| instrument.instrument_type == InstrumentType::Binance as i32 && *end_ms == 1_700_000_060_000)
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        let service = BinanceSwapClientHistoryService {
            kline: Arc::new(kline_service),
            funding_rate: Arc::new(funding_rate_service),
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
                symbol_type: "SWAP".to_string(),
                on_board_time: Some(1_700_000_000_000),
            })),
        };

        service.initial_data(instrument, 1_700_000_060_000).await.unwrap();
    }
}

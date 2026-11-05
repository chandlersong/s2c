use crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING;
use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::{
    get_binance_kline_batch_insert, get_binance_order_book_snapshot_batch_insert, get_binance_swap_funding_rate_batch_insert,
    get_binance_swap_kline_batch_insert, get_binance_trade_batch_insert,
};
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::binance::{
    BinanceInstrumentType, LocalBinanceFundingRatePo, LocalBinanceInstrumentPo, LocalBinanceKlinePo, LocalBinanceOrderBookSnapshotPo,
    LocalBinanceTradePo,
};
use crate::sync::client::repository::binance::{ClientBinanceRepository, ClientBinanceRepositoryImpl};
use crate::sync::client::sync_client_service::{
    ClientHistoryService, ClientHistoryServiceTrait, ClientInstrumentService, ClientInstrumentServiceTrait,
};
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, InstrumentType, SyncRequest, history_entry::Payload, instrument};
use async_trait::async_trait;
use futures::StreamExt;
use futures::executor::block_on;
use li::tools::time::UnixTimeStamp;
use log::{debug, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use yue::models::{DEFAULT_START_TIMESTAMP, HistoryInterval};

type InstrumentCache = Arc<RwLock<HashMap<u64, LocalBinanceInstrumentPo>>>;

async fn find_local_instrument(
    repository: &ClientBinanceRepository,
    inst_cache: &InstrumentCache,
    server_id: u64,
) -> Result<Option<LocalBinanceInstrumentPo>, YuError> {
    if let Some(instrument) = inst_cache.read().await.get(&server_id).cloned() {
        return Ok(Some(instrument));
    }

    let instrument = repository.find_instrument_by_server_id(server_id).await?;
    if let Some(instrument) = &instrument {
        inst_cache.write().await.insert(server_id, instrument.clone());
    }
    Ok(instrument)
}

pub struct BinanceSpotClientInstrumentService {
    history_services: HashMap<HistoryType, ClientHistoryService>,
    inst_cache: InstrumentCache,
    repository: ClientBinanceRepository,
}

impl BinanceSpotClientInstrumentService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        let repository = ClientBinanceRepositoryImpl::from_pool(pool.clone());
        let spot_instruments = block_on(repository.list_all_instruments(BinanceInstrumentType::Spot))?;
        let swap_instruments = block_on(repository.list_all_instruments(BinanceInstrumentType::Swap))?;
        let inst_cache = spot_instruments
            .into_iter()
            .chain(swap_instruments)
            .map(|instrument| (instrument.server_id, instrument))
            .collect();
        let inst_cache = Arc::new(RwLock::new(inst_cache));
        let kline_service = BinanceSpotKlineClientHistoryService::for_production(repository.clone(), pool.clone(), inst_cache.clone());
        let trade_service = BinanceSpotTradeClientHistoryService::for_production(repository.clone(), inst_cache.clone());
        let order_book_snapshot_service = BinanceOrderBookSnapshotClientHistoryService::for_production(repository.clone(), inst_cache.clone());
        let swap_kline_service = BinanceSwapKlineClientHistoryService::for_production(repository.clone(), pool.clone(), inst_cache.clone());
        let swap_funding_rate_service =
            BinanceSwapFundingRateClientHistoryService::for_production(repository.clone(), pool.clone(), inst_cache.clone());
        let swap_service: ClientHistoryService = Arc::new(BinanceSwapClientHistoryService {
            kline: swap_kline_service,
            funding_rate: swap_funding_rate_service.clone(),
        });
        Ok(Arc::new(Self {
            history_services: HashMap::from([
                (HistoryType::BinanceSpot, kline_service),
                (HistoryType::BinanceTrade, trade_service),
                (HistoryType::BinanceOrderBookSnapshot, order_book_snapshot_service),
                (HistoryType::BinanceSwap, swap_service),
                (HistoryType::BinanceSwapFundingRate, swap_funding_rate_service),
            ]),
            inst_cache,
            repository,
        }))
    }
}

#[async_trait]
impl ClientInstrumentServiceTrait for BinanceSpotClientInstrumentService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload.as_ref() else {
            return Err(YuError::new("instrument payload is not a Binance instrument"));
        };
        //这里其实主要更新的是kline历史数据。funding rate是swap的，暂时不管
        let history_type = match BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type) {
            Some(BinanceInstrumentType::Spot) => HistoryType::BinanceSpot,
            Some(BinanceInstrumentType::Swap) => HistoryType::BinanceSwap,
            None => return Err(YuError::new("unsupported Binance instrument symbol type")),
        };
        self.history_services
            .get(&history_type)
            .ok_or_else(|| YuError::new("Binance history service is not configured"))?
            .initial_data(instrument.clone(), end_ms)
            .await?;

        //swap还要更新一下funding rate
        if history_type == HistoryType::BinanceSwap {
            let funding_rate_service = self.history_services.get(&HistoryType::BinanceSwapFundingRate);
            if let Some(fr_service) = funding_rate_service {
                if let Err(e) = fr_service.initial_data(instrument.clone(), end_ms).await {
                    warn!(
                        "Error initializing Binance swap funding rate history, server id:{},error is {}",
                        binance_instrument.server_id, e
                    );
                }
            }
        }

        // FUTURE：
        // trade这类初始化的逻辑，就很难，因为他不是全集,需要服务器把这类信息发过来。所以就暂时先不管了。。
        Ok(())
    }

    ///
    /// 1. 把instrument转换成LocalBinanceInstrumentPo
    /// 2. 判断instrument是否存在，如果存在，判断是否一致，不一致就更新。
    /// 3. 如果不存在，就创建新的instrument,然后插入cache
    ///
    /// FUTURE:
    /// 1. cache是否要考虑删除？现在没有删除机制。
    ///
    async fn reconcile_instrument(&self, instrument: Instrument) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Binance as i32 {
            return Err(YuError::new("instrument exchange is not Binance"));
        }
        let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not Binance"));
        };
        if BinanceInstrumentType::from_symbol_type(&binance_instrument.symbol_type).is_none() {
            return Err(YuError::new("unsupported Binance instrument symbol type"));
        }

        let mut local_instrument = LocalBinanceInstrumentPo::from_proto(binance_instrument.clone());
        let cached_instrument = self.inst_cache.read().await.get(&binance_instrument.server_id).cloned();
        match cached_instrument {
            Some(existing_instrument) => {
                local_instrument.id = existing_instrument.id;
                let changed = existing_instrument.symbol != local_instrument.symbol
                    || existing_instrument.status != local_instrument.status
                    || existing_instrument.base_asset != local_instrument.base_asset
                    || existing_instrument.quote_asset != local_instrument.quote_asset
                    || existing_instrument.quote_asset_precision != local_instrument.quote_asset_precision
                    || existing_instrument.order_types != local_instrument.order_types
                    || existing_instrument.symbol_type != local_instrument.symbol_type
                    || existing_instrument.on_board_time != local_instrument.on_board_time;
                if changed {
                    self.repository.update_instrument(local_instrument.clone()).await?;
                }
                if local_instrument.status == BN_SYMBOL_STATUS_TRADING {
                    if changed {
                        self.inst_cache.write().await.insert(binance_instrument.server_id, local_instrument);
                    }
                } else {
                    self.inst_cache.write().await.remove(&binance_instrument.server_id);
                }
            }
            None => {
                local_instrument.id = self.repository.create_instrument(local_instrument.clone()).await?;
                if local_instrument.status == BN_SYMBOL_STATUS_TRADING {
                    self.inst_cache.write().await.insert(binance_instrument.server_id, local_instrument);
                }
            }
        }

        Ok(())
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
    inst_cache: InstrumentCache,
}

impl BinanceSwapKlineClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, inst_cache: InstrumentCache) -> ClientHistoryService {
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
            inst_cache,
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, binance_instrument.server_id).await? else {
            debug!("local Binance instrument {} not found; skipping history", binance_instrument.server_id);
            return Ok(());
        };

        let local_inst_id = local_instrument.id;

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

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
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
                0,
            )?
            .gaps;
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            debug!("local Binance instrument {} not found; skipping history", history_entry.inst_id);
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
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

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        self.kline.reconcile_history_data(end_ms).await
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type == HistoryType::BinanceSwapFundingRate as i32 {
            self.funding_rate.insert_history(history_entry).await
        } else {
            self.kline.insert_history(history_entry).await
        }
    }
}

pub struct BinanceSwapFundingRateClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceFundingRatePo>,
    repository: ClientBinanceRepository,
    interval: HistoryInterval,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    inst_cache: InstrumentCache,
}

impl BinanceSwapFundingRateClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, inst_cache: InstrumentCache) -> ClientHistoryService {
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
            inst_cache,
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, binance_instrument.server_id).await? else {
            debug!("local Binance instrument {} not found; skipping history", binance_instrument.server_id);
            return Ok(());
        };

        let local_inst_id = local_instrument.id;

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

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
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
                0,
            )?
            .gaps;
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            debug!(
                "local Binance instrument {} not found; skipping funding rate history",
                history_entry.inst_id
            );
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
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
    inst_cache: InstrumentCache,
}

impl BinanceSpotKlineClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, pool: sqlx::PgPool, inst_cache: InstrumentCache) -> ClientHistoryService {
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
            inst_cache,
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, binance_instrument.server_id).await? else {
            debug!("local Binance instrument {} not found; skipping history", binance_instrument.server_id);
            return Ok(());
        };

        let local_inst_id = local_instrument.id;

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

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        for instrument in self.repository.list_all_instruments(BinanceInstrumentType::Spot).await? {
            if !instrument.status.eq_ignore_ascii_case("TRADING") {
                continue;
            }
            // FUTURE: 以后通过获取服务器端的时间来确定。现在不存在，为0
            let on_board_time = instrument.on_board_time.unwrap_or_else(|| DEFAULT_START_TIMESTAMP);
            let start_timestamp = self.first_candle_timestamp(on_board_time);
            let gaps = binary_search_gap(
                &instrument.server_id.to_string(),
                "SPOT",
                start_timestamp,
                end_timestamp,
                &self.interval,
                self.binary_search.clone(),
                0,
            )?
            .gaps;
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

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            debug!("local Binance instrument {} not found; skipping history", history_entry.inst_id);
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
}

pub struct BinanceSpotTradeClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceTradePo>,
    repository: ClientBinanceRepository,
    inst_cache: InstrumentCache,
}

impl BinanceSpotTradeClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, inst_cache: InstrumentCache) -> ClientHistoryService {
        Arc::new(Self {
            history_batch_insert: get_binance_trade_batch_insert(),
            repository,
            inst_cache,
        })
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceSpotTradeClientHistoryService {
    async fn initial_data(&self, _instrument: Instrument, _end_ms: UnixTimeStamp) -> Result<(), YuError> {
        // FUTURE: Binance spot trade history恢复保存到数据库的时候再说吧。.
        Ok(())
    }

    async fn reconcile_history_data(&self, _end_ms: UnixTimeStamp) -> Result<(), YuError> {
        // FUTURE: Binance spot trade history reconciliation，逻辑上要重写，通过id。先不管了。。
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::BinanceTrade as i32
            || history_entry.instrument_type != InstrumentType::Binance as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::BinanceTrade(_)))
        {
            return Err(YuError::new("received non-Binance spot trade history payload"));
        }

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            debug!("local Binance instrument {} not found; skipping trade history", history_entry.inst_id);
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
}

pub struct BinanceOrderBookSnapshotClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalBinanceOrderBookSnapshotPo>,
    repository: ClientBinanceRepository,
    inst_cache: InstrumentCache,
}

impl BinanceOrderBookSnapshotClientHistoryService {
    fn for_production(repository: ClientBinanceRepository, inst_cache: InstrumentCache) -> ClientHistoryService {
        Arc::new(Self {
            history_batch_insert: get_binance_order_book_snapshot_batch_insert(),
            repository,
            inst_cache,
        })
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for BinanceOrderBookSnapshotClientHistoryService {
    async fn initial_data(&self, _instrument: Instrument, _end_ms: UnixTimeStamp) -> Result<(), YuError> {
        Ok(())
    }

    async fn reconcile_history_data(&self, _end_ms: UnixTimeStamp) -> Result<(), YuError> {
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::BinanceOrderBookSnapshot as i32
            || history_entry.instrument_type != InstrumentType::Binance as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::BinanceOrderBookSnapshot(_)))
        {
            return Err(YuError::new("received non-Binance order book snapshot history payload"));
        }

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            debug!(
                "local Binance instrument {} not found; skipping order book snapshot history",
                history_entry.inst_id
            );
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
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
    use crate::sync::client::sync_client_service::MockClientHistoryServiceTrait;
    use crate::sync::models::grpc_sync::{
        BinanceFundingRate, BinanceKline, BinanceOrderBookLevel, BinanceOrderBookSnapshot, BinanceTrade, HistoryType, InstrumentType,
    };

    fn mock_binary_search() -> BinarySearchDS {
        Arc::new(MockBinarySearchDSTrait::new())
    }

    fn local_instrument(server_id: u64, id: u64) -> LocalBinanceInstrumentPo {
        LocalBinanceInstrumentPo {
            id,
            server_id,
            symbol: "BTCUSDT".to_string(),
            status: "TRADING".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            symbol_type: "SPOT".to_string(),
            on_board_time: Some(1_700_000_000_000),
        }
    }

    fn instrument_cache(cached: Option<(u64, u64)>) -> InstrumentCache {
        Arc::new(RwLock::new(
            cached
                .map(|(server_id, id)| HashMap::from([(server_id, local_instrument(server_id, id))]))
                .unwrap_or_default(),
        ))
    }

    fn server_instrument() -> Instrument {
        Instrument {
            instrument_type: InstrumentType::Binance as i32,
            payload: Some(instrument::Payload::Binance(crate::sync::models::grpc_sync::BinanceInstrument {
                server_id: 42,
                symbol: "BTCUSDT".to_string(),
                status: "TRADING".to_string(),
                base_asset: "BTC".to_string(),
                quote_asset: "USDT".to_string(),
                quote_asset_precision: 8,
                order_types: vec!["LIMIT".to_string()],
                symbol_type: "SPOT".to_string(),
                on_board_time: Some(1_700_000_000_000),
            })),
        }
    }

    fn order_book_snapshot_entry() -> HistoryEntry {
        HistoryEntry {
            history_type: HistoryType::BinanceOrderBookSnapshot as i32,
            instrument_type: InstrumentType::Binance as i32,
            inst_id: 42,
            payload: Some(Payload::BinanceOrderBookSnapshot(BinanceOrderBookSnapshot {
                server_id: 99,
                snapshot_time: 1_700_000_000_123,
                local_update_id: 1234,
                bids: vec![BinanceOrderBookLevel {
                    price: "100.00000001".to_string(),
                    quantity: "2.5".to_string(),
                }],
                asks: vec![BinanceOrderBookLevel {
                    price: "100.00000002".to_string(),
                    quantity: "1.25".to_string(),
                }],
            })),
        }
    }

    // 测试目的：验证实时订单簿快照使用缓存的本地 instrument ID 提交批量落库。
    // 准备数据/过程：缓存 server ID 42 到 local ID 7，并提交含 bid/ask 档位的合法快照。
    // 预期结果：batch insert 收到原始快照消息一次，且使用本地 instrument ID 7。
    #[tokio::test]
    async fn order_book_snapshot_insert_uses_cached_local_instrument_id() {
        let repository = MockClientBinanceRepositoryTrait::new();
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceOrderBookSnapshotPo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.inst_id == 42
                    && *local_inst_id == 7
                    && matches!(
                        entry.payload.as_ref(),
                        Some(Payload::BinanceOrderBookSnapshot(snapshot))
                            if snapshot.server_id == 99
                                && snapshot.snapshot_time == 1_700_000_000_123
                                && snapshot.local_update_id == 1234
                                && snapshot.bids.first().is_some_and(|level| level.price == "100.00000001" && level.quantity == "2.5")
                                && snapshot.asks.first().is_some_and(|level| level.price == "100.00000002" && level.quantity == "1.25")
                    )
            })
            .times(1)
            .returning(|_, _| ());
        let service = BinanceOrderBookSnapshotClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(Some((42, 7))),
        };

        service.insert_history(order_book_snapshot_entry()).await.unwrap();
    }

    // 测试目的：验证缓存未命中时按服务端 instrument ID 查询本地 ID，并缓存映射供后续快照复用。
    // 准备数据/过程：mock repository 返回 local ID 7，连续提交两条相同 instrument 的合法快照。
    // 预期结果：repository 只查询一次，两个快照都以 local ID 7 提交 batch insert。
    #[tokio::test]
    async fn order_book_snapshot_insert_resolves_and_caches_local_instrument_id() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository
            .expect_find_instrument_by_server_id()
            .with(mockall::predicate::eq(42))
            .times(1)
            .returning(|_| Ok(Some(local_instrument(42, 7))));
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceOrderBookSnapshotPo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| entry.history_type == HistoryType::BinanceOrderBookSnapshot as i32 && *local_inst_id == 7)
            .times(2)
            .returning(|_, _| ());
        let service = BinanceOrderBookSnapshotClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(None),
        };

        service.insert_history(order_book_snapshot_entry()).await.unwrap();
        service.insert_history(order_book_snapshot_entry()).await.unwrap();
    }

    // 测试目的：验证本地不存在对应 instrument 时不会把服务端 ID 错当成本地 ID 落库。
    // 准备数据/过程：mock repository 对 server ID 42 返回 None，并提交一条合法快照。
    // 预期结果：处理成功跳过该消息，且不调用 batch insert。
    #[tokio::test]
    async fn order_book_snapshot_insert_skips_unknown_local_instrument() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository
            .expect_find_instrument_by_server_id()
            .with(mockall::predicate::eq(42))
            .times(1)
            .returning(|_| Ok(None));
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceOrderBookSnapshotPo>::new();
        batch_insert.expect_insert_data().times(0);
        let service = BinanceOrderBookSnapshotClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(None),
        };

        service.insert_history(order_book_snapshot_entry()).await.unwrap();
    }

    // 测试目的：验证错误的 history type 不会进入 instrument 查询或订单簿落库流程。
    // 准备数据/过程：将合法快照消息的 history type 改为 BinanceTrade 后提交。
    // 预期结果：返回校验错误，且 repository 和 batch insert 均不被调用。
    #[tokio::test]
    async fn order_book_snapshot_insert_rejects_mismatched_history_type() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceOrderBookSnapshotPo>::new();
        batch_insert.expect_insert_data().times(0);
        let service = BinanceOrderBookSnapshotClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(Some((42, 7))),
        };
        let mut entry = order_book_snapshot_entry();
        entry.history_type = HistoryType::BinanceTrade as i32;

        let error = service.insert_history(entry).await.unwrap_err();

        assert!(error.to_string().contains("non-Binance order book snapshot"));
    }

    // 测试目的：验证订单簿快照服务不执行历史初始化或缺口对账，只负责实时接收与落地。
    // 准备数据/过程：使用空 repository 调用 initial_data 和 reconcile_living_instrument_with_server。
    // 预期结果：两个操作均正常返回，不触发远端回补或数据库查询。
    #[tokio::test]
    async fn order_book_snapshot_initialization_and_reconciliation_are_noops() {
        let repository = MockClientBinanceRepositoryTrait::new();
        let batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceOrderBookSnapshotPo>::new();
        let service = BinanceOrderBookSnapshotClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(None),
        };

        service
            .initial_data(
                Instrument {
                    instrument_type: InstrumentType::Binance as i32,
                    payload: None,
                },
                1_700_000_000_000,
            )
            .await
            .unwrap();
        service.reconcile_history_data(1_700_000_000_000).await.unwrap();
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
            inst_cache: instrument_cache(Some((42, 7))),
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
            inst_cache: instrument_cache(Some((42, 7))),
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
            .expect_find_instrument_by_server_id()
            .with(mockall::predicate::eq(42))
            .times(1)
            .returning(|_| Ok(Some(local_instrument(42, 7))));
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
            inst_cache: instrument_cache(None),
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
            inst_cache: instrument_cache(Some((42, 7))),
        };

        service.reconcile_history_data(end_timestamp).await.unwrap();
    }

    // 测试目的：验证 Binance Spot Trade 实时历史使用缓存的服务端到本地 instrument ID 映射。
    // 准备数据/过程：缓存 server ID 42 到 local ID 7，并提交一条合法的 BinanceTrade history entry。
    // 预期结果：batch insert 被调用一次，且接收本地 instrument ID 7 和原始交易内容。
    #[tokio::test]
    async fn spot_trade_insert_history_uses_cached_local_instrument_id() {
        let repository = MockClientBinanceRepositoryTrait::new();
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalBinanceTradePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.history_type == HistoryType::BinanceTrade as i32
                    && entry.inst_id == 42
                    && *local_inst_id == 7
                    && matches!(entry.payload.as_ref(), Some(Payload::BinanceTrade(trade)) if trade.trade_id == 99)
            })
            .times(1)
            .returning(|_, _| ());

        let service = BinanceSpotTradeClientHistoryService {
            history_batch_insert: Arc::new(batch_insert),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(Some((42, 7))),
        };
        service
            .insert_history(HistoryEntry {
                history_type: HistoryType::BinanceTrade as i32,
                instrument_type: InstrumentType::Binance as i32,
                inst_id: 42,
                payload: Some(Payload::BinanceTrade(BinanceTrade {
                    id: 1,
                    event_time: 1_700_000_000_001,
                    trade_id: 99,
                    price: 100.5,
                    qty: 0.25,
                    trade_time: Some(1_700_000_000_000),
                    is_buyer_maker: Some(true),
                    created_at: 1_700_000_000_002,
                })),
            })
            .await
            .unwrap();
    }

    // 测试目的：验证服务端缺少的 Binance instrument 会被创建并写入共享缓存。
    // 准备数据/过程：缓存中不含 server ID 42，并 mock 创建操作返回本地 ID 7。
    // 预期结果：创建内容保留服务端字段，缓存记录 server ID 42 到本地 ID 7 的 instrument。
    #[tokio::test]
    async fn reconcile_instrument_creates_missing_instrument_and_caches_it() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_create_instrument()
            .withf(|instrument| {
                instrument.server_id == 42 && instrument.symbol == "BTCUSDT" && instrument.status == "TRADING" && instrument.symbol_type == "SPOT"
            })
            .times(1)
            .returning(|_| Ok(7));
        repository.expect_update_instrument().times(0);

        let cache = instrument_cache(None);
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: cache.clone(),
            repository: Arc::new(repository),
        };

        service.reconcile_instrument(server_instrument()).await.unwrap();

        let cached = cache.read().await.get(&42).cloned().unwrap();
        assert_eq!(cached.id, 7);
        assert_eq!(cached.symbol, "BTCUSDT");
    }

    // 测试目的：验证新建的非交易中 Binance instrument 不会进入共享缓存。
    // 准备数据/过程：提交状态为 BREAK 的 instrument，并 mock 创建操作返回本地 ID 7。
    // 预期结果：instrument 成功写入 repository，但 server ID 42 不在缓存中。
    #[tokio::test]
    async fn reconcile_instrument_does_not_cache_non_trading_instrument() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_create_instrument()
            .withf(|instrument| instrument.server_id == 42 && instrument.status == "BREAK")
            .times(1)
            .returning(|_| Ok(7));
        repository.expect_update_instrument().times(0);
        let cache = instrument_cache(None);
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: cache.clone(),
            repository: Arc::new(repository),
        };
        let mut instrument = server_instrument();
        if let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload.as_mut() {
            binance_instrument.status = "BREAK".to_string();
        }

        service.reconcile_instrument(instrument).await.unwrap();

        assert!(!cache.read().await.contains_key(&42));
    }

    // 测试目的：验证本地 Binance instrument 字段一致时不执行数据库写入。
    // 准备数据/过程：预热与服务端 instrument 完全一致的缓存，并禁止调用 repository 查询、创建或更新。
    // 预期结果：对账成功且本地 instrument ID 和字段保持不变。
    #[tokio::test]
    async fn reconcile_instrument_skips_write_when_cached_instrument_matches() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository.expect_create_instrument().times(0);
        repository.expect_update_instrument().times(0);
        let cache = instrument_cache(Some((42, 7)));
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: cache.clone(),
            repository: Arc::new(repository),
        };

        service.reconcile_instrument(server_instrument()).await.unwrap();

        let cached = cache.read().await.get(&42).cloned().unwrap();
        assert_eq!(cached.id, 7);
        assert_eq!(cached.status, "TRADING");
    }

    // 测试目的：验证发现 Binance instrument 字段变化时保留本地 ID、更新数据库并刷新缓存。
    // 准备数据/过程：缓存中预置状态为 BREAK 的本地 instrument，服务端状态为 TRADING，并校验更新 PO。
    // 预期结果：update 被调用一次，且缓存包含更新后的字段和原本地 ID。
    #[tokio::test]
    async fn reconcile_instrument_updates_changed_instrument() {
        let mut cached = local_instrument(42, 7);
        cached.status = "BREAK".to_string();
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_update_instrument()
            .withf(|instrument| instrument.id == 7 && instrument.server_id == 42 && instrument.status == "TRADING")
            .times(1)
            .returning(|_| Ok(()));
        repository.expect_create_instrument().times(0);

        let cache = Arc::new(RwLock::new(HashMap::from([(42, cached)])));
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: cache.clone(),
            repository: Arc::new(repository),
        };

        service.reconcile_instrument(server_instrument()).await.unwrap();

        let cached = cache.read().await.get(&42).cloned().unwrap();
        assert_eq!(cached.id, 7);
        assert_eq!(cached.status, "TRADING");
    }

    // 测试目的：验证缓存中的 Binance instrument 变为非交易状态后会从共享缓存移除。
    // 准备数据/过程：预置交易中 instrument，并提交状态为 BREAK 的服务端 instrument。
    // 预期结果：repository 更新一次，且 server ID 42 从缓存中移除。
    #[tokio::test]
    async fn reconcile_instrument_removes_non_trading_instrument_from_cache() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_update_instrument()
            .withf(|instrument| instrument.id == 7 && instrument.server_id == 42 && instrument.status == "BREAK")
            .times(1)
            .returning(|_| Ok(()));
        repository.expect_create_instrument().times(0);
        let cache = instrument_cache(Some((42, 7)));
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: cache.clone(),
            repository: Arc::new(repository),
        };
        let mut instrument = server_instrument();
        if let Some(instrument::Payload::Binance(binance_instrument)) = instrument.payload.as_mut() {
            binance_instrument.status = "BREAK".to_string();
        }

        service.reconcile_instrument(instrument).await.unwrap();

        assert!(!cache.read().await.contains_key(&42));
    }

    // 测试目的：验证其他交易所的 instrument 不会进入 Binance instrument 对账流程。
    // 准备数据/过程：将 Binance instrument 的 instrument_type 改为 OKX，并配置 repository 无可调用操作。
    // 预期结果：返回 exchange 校验错误，且 repository 不被访问。
    #[tokio::test]
    async fn reconcile_instrument_rejects_another_exchange() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository.expect_create_instrument().times(0);
        repository.expect_update_instrument().times(0);
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: instrument_cache(None),
            repository: Arc::new(repository),
        };
        let mut instrument = server_instrument();
        instrument.instrument_type = InstrumentType::Okx as i32;

        let error = service.reconcile_instrument(instrument).await.unwrap_err();

        assert!(error.to_string().contains("exchange is not Binance"));
    }

    // 测试目的：验证缺失 Binance payload 或不支持的 symbol type 会被明确拒绝。
    // 准备数据/过程：分别提交 payload 为空和 symbol_type 为 OPTION 的 instrument，并禁止 repository 调用。
    // 预期结果：两个请求均返回对应校验错误，不读写本地 instrument。
    #[tokio::test]
    async fn reconcile_instrument_rejects_invalid_payload_and_symbol_type() {
        let mut repository = MockClientBinanceRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository.expect_create_instrument().times(0);
        repository.expect_update_instrument().times(0);
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::new(),
            inst_cache: instrument_cache(None),
            repository: Arc::new(repository),
        };
        let mut missing_payload = server_instrument();
        missing_payload.payload = None;
        let mut unsupported_symbol = server_instrument();
        if let Some(instrument::Payload::Binance(binance_instrument)) = unsupported_symbol.payload.as_mut() {
            binance_instrument.symbol_type = "OPTION".to_string();
        }

        let payload_error = service.reconcile_instrument(missing_payload).await.unwrap_err();
        let symbol_type_error = service.reconcile_instrument(unsupported_symbol).await.unwrap_err();

        assert!(payload_error.to_string().contains("payload is not Binance"));
        assert!(symbol_type_error.to_string().contains("unsupported Binance instrument symbol type"));
    }

    // 测试目的：验证 Binance Swap instrument 被路由到 Swap history service，而不误调 Spot service。
    // 准备数据/过程：仅注册 Swap mock service，并向 instrument service 提交 symbol_type 为 SWAP 的 Binance instrument。
    // 预期结果：Swap service 收到一次该 instrument，调用成功。
    #[tokio::test]
    async fn initial_data_routes_swap_instrument_to_swap_service() {
        let mut swap_history_service = MockClientHistoryServiceTrait::new();
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
            inst_cache: instrument_cache(None),
            repository: Arc::new(MockClientBinanceRepositoryTrait::new()),
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

    // 测试目的：验证 Spot instrument 初始化同时执行 Kline 和 Trade 历史同步。
    // 准备数据/过程：分别注册 Spot Kline 与 BinanceTrade mock service，并提交 symbol_type 为 SPOT 的 instrument。
    // 预期结果：两个 service 均收到同一 instrument 和结束时间各一次，且初始化成功。
    #[tokio::test]
    async fn initial_data_routes_spot_instrument_to_kline_and_trade_services() {
        let mut kline_service = MockClientHistoryServiceTrait::new();
        kline_service
            .expect_initial_data()
            .withf(|instrument, end_ms| {
                instrument.instrument_type == InstrumentType::Binance as i32
                    && *end_ms == 1_700_000_060_000
                    && matches!(instrument.payload.as_ref(), Some(instrument::Payload::Binance(value)) if value.symbol_type == "SPOT")
            })
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        let mut trade_service = MockClientHistoryServiceTrait::new();
        trade_service
            .expect_initial_data()
            .withf(|instrument, end_ms| {
                instrument.instrument_type == InstrumentType::Binance as i32
                    && *end_ms == 1_700_000_060_000
                    && matches!(instrument.payload.as_ref(), Some(instrument::Payload::Binance(value)) if value.symbol_type == "SPOT")
            })
            .times(0)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        let service = BinanceSpotClientInstrumentService {
            history_services: HashMap::from([
                (HistoryType::BinanceSpot, Arc::new(kline_service) as ClientHistoryService),
                (HistoryType::BinanceTrade, Arc::new(trade_service) as ClientHistoryService),
            ]),
            inst_cache: instrument_cache(None),
            repository: Arc::new(MockClientBinanceRepositoryTrait::new()),
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
                symbol_type: "SPOT".to_string(),
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

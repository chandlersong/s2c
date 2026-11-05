use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::{get_okx_kline_batch_insert, get_okx_option_summary_batch_insert};
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxKlinePo, LocalOkxOptionSummaryPo};
use crate::sync::client::repository::okx::{ClientOkxRepository, ClientOkxRepositoryImpl};
use crate::sync::client::sync_client_service::{
    ClientHistoryService, ClientHistoryServiceTrait, ClientInstrumentService, ClientInstrumentServiceTrait,
};
use crate::sync::models::grpc_sync::{
    HistoryEntry, HistoryType, Instrument, InstrumentType, OkxInstrument, SyncRequest, history_entry::Payload, instrument,
};
use async_trait::async_trait;
use futures::StreamExt;
use futures::executor::block_on;
use li::tools::time::UnixTimeStamp;
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use yue::models::HistoryInterval;

type InstrumentCache = Arc<RwLock<HashMap<u64, LocalOkxInstrumentPo>>>;
const OKX_INSTRUMENT_STATE_LIVE: &str = "live";

async fn find_local_instrument(
    repository: &ClientOkxRepository,
    inst_cache: &InstrumentCache,
    server_id: u64,
) -> Result<Option<LocalOkxInstrumentPo>, YuError> {
    if let Some(instrument) = inst_cache.read().await.get(&server_id).cloned() {
        return Ok(Some(instrument));
    }

    let instrument = repository.find_instrument_by_server_id(server_id).await?;
    if let Some(instrument) = &instrument {
        inst_cache.write().await.insert(server_id, instrument.clone());
    }
    Ok(instrument)
}

async fn find_or_create_local_instrument(
    repository: &ClientOkxRepository,
    inst_cache: &InstrumentCache,
    server_instrument: OkxInstrument,
) -> Result<LocalOkxInstrumentPo, YuError> {
    if let Some(instrument) = find_local_instrument(repository, inst_cache, server_instrument.server_id).await? {
        return Ok(instrument);
    }

    let mut instrument = LocalOkxInstrumentPo::from_proto(server_instrument);
    instrument.id = repository.create_instruments(instrument.clone()).await?;
    inst_cache.write().await.insert(instrument.server_id, instrument.clone());
    Ok(instrument)
}

pub struct OkxHistoryClientService {
    history_services: HashMap<HistoryType, ClientHistoryService>,
    repository: ClientOkxRepository,
    inst_cache: InstrumentCache,
}

impl OkxHistoryClientService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        let repository = ClientOkxRepositoryImpl::from_pool(pool.clone());
        let inst_cache = Arc::new(RwLock::new(block_on(repository.server_id_instrument_dictionary())?));
        let kline_history_service = OkxKlineClientHistoryService::for_production(repository.clone(), pool, inst_cache.clone());
        let option_summary_history_service = OkxOptionSummaryClientHistoryService::for_production(repository.clone(), inst_cache.clone());
        let history_services = HashMap::from([
            (HistoryType::OkxKline, kline_history_service),
            (HistoryType::OkxOptionSummary, option_summary_history_service),
        ]);
        Ok(Arc::new(Self {
            history_services,
            repository,
            inst_cache,
        }))
    }
}

#[async_trait]
impl ClientInstrumentServiceTrait for OkxHistoryClientService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        for history_service in self.history_services.values() {
            history_service.initial_data(instrument.clone(), end_ms).await?;
        }
        Ok(())
    }

    async fn reconcile_instrument(&self, instrument: Instrument) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Okx as i32 {
            return Err(YuError::new("instrument exchange is not OKX"));
        }
        let Some(instrument::Payload::Okx(okx_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not OKX"));
        };

        let mut local_instrument = LocalOkxInstrumentPo::from_proto(okx_instrument.clone());
        let cached_instrument = self.inst_cache.read().await.get(&okx_instrument.server_id).cloned();
        match cached_instrument {
            Some(existing_instrument) => {
                local_instrument.id = existing_instrument.id;
                let changed = local_instrument != existing_instrument;
                if changed {
                    self.repository.update_instrument(local_instrument.clone()).await?;
                }
                if local_instrument
                    .state
                    .as_deref()
                    .is_some_and(|state| state.eq_ignore_ascii_case(OKX_INSTRUMENT_STATE_LIVE))
                {
                    if changed {
                        self.inst_cache.write().await.insert(okx_instrument.server_id, local_instrument);
                    }
                } else {
                    self.inst_cache.write().await.remove(&okx_instrument.server_id);
                }
            }
            None => {
                local_instrument.id = self.repository.create_instruments(local_instrument.clone()).await?;
                if local_instrument
                    .state
                    .as_deref()
                    .is_some_and(|state| state.eq_ignore_ascii_case(OKX_INSTRUMENT_STATE_LIVE))
                {
                    self.inst_cache.write().await.insert(okx_instrument.server_id, local_instrument);
                }
            }
        }
        Ok(())
    }

    fn supported_history_services(&self) -> Result<HashMap<HistoryType, ClientHistoryService>, YuError> {
        Ok(self.history_services.clone())
    }
}

pub struct OkxKlineClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalOkxKlinePo>,
    repository: ClientOkxRepository,
    interval: HistoryInterval,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    inst_cache: InstrumentCache,
}

impl OkxKlineClientHistoryService {
    fn first_candle_timestamp(&self, list_time: u64) -> u64 {
        let interval_ms = self.interval.to_milliseconds();
        let aligned_list_time = self.interval.get_close_unix_ms(list_time);
        if aligned_list_time < list_time {
            aligned_list_time.saturating_add(interval_ms)
        } else {
            aligned_list_time
        }
    }

    fn for_production(repository: ClientOkxRepository, pool: sqlx::PgPool, inst_cache: InstrumentCache) -> ClientHistoryService {
        let history_batch_insert = get_okx_kline_batch_insert();
        let binary_search = SyncClientBinarySearchDataImpl::new(
            pool,
            ClientsTables::OkxPriceHistory.table_name(),
            ClientsTables::OkxInstruments.table_name(),
            "timestamp",
        );
        Arc::new(Self {
            history_batch_insert,
            repository,
            interval: HistoryInterval::OneMinute,
            grpc_manager: get_grpc_manager_from_config(),
            binary_search,
            inst_cache,
        })
    }

    async fn backfill_history(&self, server_inst_id: u64, local_inst_id: u64, start_ms: u64, end_ms: u64) -> Result<(), YuError> {
        if start_ms >= end_ms {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: server_inst_id,
                start_ms,
                end_ms,
                history_type: HistoryType::OkxKline as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            for entry in message?.history {
                if entry.history_type != HistoryType::OkxKline as i32
                    || entry.instrument_type != InstrumentType::Okx as i32
                    || entry.inst_id != server_inst_id
                    || !matches!(entry.payload.as_ref(), Some(Payload::OkxKline(_)))
                {
                    return Err(YuError::new("received invalid OKX kline history entry"));
                }
                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for OkxKlineClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Okx as i32 {
            return Err(YuError::new("instrument exchange is not OKX"));
        }
        let Some(instrument::Payload::Okx(okx_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not OKX"));
        };

        let local_inst_id = find_or_create_local_instrument(&self.repository, &self.inst_cache, okx_instrument.clone())
            .await?
            .id;

        let latest_timestamp = self.repository.find_latest_kline_timestamp_by_server_id(okx_instrument.server_id).await?;
        let start_ms = match latest_timestamp {
            Some(timestamp) => timestamp,
            None if okx_instrument.list_time > 0 => self.first_candle_timestamp(okx_instrument.list_time).saturating_sub(1),
            None => return Err(YuError::new("OKX instrument listing time is unavailable")),
        };
        let end_ms = self.interval.get_close_unix_ms(end_ms);
        self.backfill_history(okx_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        let instruments = self.repository.list_all_instrument().await?;

        for instrument in instruments {
            if !instrument
                .state
                .as_deref()
                .is_some_and(|state| state.eq_ignore_ascii_case(OKX_INSTRUMENT_STATE_LIVE))
            {
                continue;
            }
            let Some(list_time) = instrument.list_time else {
                warn!(
                    "OKX instrument {} has no listing time; skipping history reconciliation",
                    instrument.server_id
                );
                continue;
            };
            let start_timestamp = self.first_candle_timestamp(list_time);
            let gaps = binary_search_gap(
                &instrument.server_id.to_string(),
                "OKX",
                start_timestamp,
                end_timestamp,
                &self.interval,
                self.binary_search.clone(),
                0,
            )?
            .gaps;
            for gap in gaps {
                if let ValidationGap::MissingData { start_time, end_time, .. } = gap {
                    self.backfill_history(instrument.server_id, instrument.id, start_time.saturating_sub(1), end_time)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::OkxKline as i32
            || history_entry.instrument_type != InstrumentType::Okx as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::OkxKline(_)))
        {
            return Err(YuError::new("received non-OKX kline history payload"));
        }

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            warn!("local OKX instrument {} not found; skipping history", history_entry.inst_id);
            return Ok(());
        };
        let local_inst_id = local_instrument.id;
        self.history_batch_insert.insert_data(history_entry, local_inst_id).await;
        Ok(())
    }
}

pub struct OkxOptionSummaryClientHistoryService {
    history_batch_insert: PostgresqlBatchInsert<LocalOkxOptionSummaryPo>,
    repository: ClientOkxRepository,
    grpc_manager: GrpcChannelManager,
    inst_cache: InstrumentCache,
}

impl OkxOptionSummaryClientHistoryService {
    fn for_production(repository: ClientOkxRepository, inst_cache: InstrumentCache) -> ClientHistoryService {
        Arc::new(Self {
            history_batch_insert: get_okx_option_summary_batch_insert(),
            repository,
            grpc_manager: get_grpc_manager_from_config(),
            inst_cache,
        })
    }

    async fn backfill_history(&self, server_inst_id: u64, local_inst_id: u64, start_ms: u64, end_ms: u64) -> Result<(), YuError> {
        if start_ms >= end_ms {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: server_inst_id,
                start_ms: start_ms.saturating_add(1),
                end_ms,
                history_type: HistoryType::OkxOptionSummary as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            for entry in message?.history {
                if entry.history_type != HistoryType::OkxOptionSummary as i32
                    || entry.instrument_type != InstrumentType::Okx as i32
                    || entry.inst_id != server_inst_id
                    || !matches!(entry.payload.as_ref(), Some(Payload::OkxOptionSummary(_)))
                {
                    return Err(YuError::new("received invalid OKX option summary history entry"));
                }
                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }

    async fn resolve_local_instrument_id(&self, instrument: &OkxInstrument) -> Result<u64, YuError> {
        Ok(find_or_create_local_instrument(&self.repository, &self.inst_cache, instrument.clone())
            .await?
            .id)
    }
}

#[async_trait]
impl ClientHistoryServiceTrait for OkxOptionSummaryClientHistoryService {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.instrument_type != InstrumentType::Okx as i32 {
            return Err(YuError::new("instrument exchange is not OKX"));
        }
        let Some(instrument::Payload::Okx(okx_instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not OKX"));
        };

        let local_inst_id = self.resolve_local_instrument_id(&okx_instrument).await?;
        let latest_timestamp = self
            .repository
            .find_latest_option_summary_timestamp_by_server_id(okx_instrument.server_id)
            .await?;
        let start_ms = match latest_timestamp {
            Some(timestamp) => timestamp,
            None if okx_instrument.list_time > 0 => okx_instrument.list_time,
            None => return Err(YuError::new("OKX instrument listing time is unavailable")),
        };

        self.backfill_history(okx_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_history_data(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let instruments = self.repository.list_all_instrument().await?;
        let latest_timestamps = self.repository.list_option_summary_timestamps().await?;

        for instrument in instruments {
            if !instrument
                .state
                .as_deref()
                .is_some_and(|state| state.eq_ignore_ascii_case(OKX_INSTRUMENT_STATE_LIVE))
            {
                continue;
            }
            let start_ms = match latest_timestamps.get(&instrument.server_id).copied() {
                Some(timestamp) => timestamp,
                None => {
                    let Some(list_time) = instrument.list_time else {
                        warn!(
                            "OKX instrument {} has no listing time; skipping option summary reconciliation",
                            instrument.server_id
                        );
                        continue;
                    };
                    list_time
                }
            };
            self.backfill_history(instrument.server_id, instrument.id, start_ms, end_ms).await?;
        }

        Ok(())
    }

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError> {
        if history_entry.history_type != HistoryType::OkxOptionSummary as i32
            || history_entry.instrument_type != InstrumentType::Okx as i32
            || !matches!(history_entry.payload.as_ref(), Some(Payload::OkxOptionSummary(_)))
        {
            return Err(YuError::new("received non-OKX option summary history payload"));
        }

        let Some(local_instrument) = find_local_instrument(&self.repository, &self.inst_cache, history_entry.inst_id).await? else {
            warn!(
                "local OKX instrument {} not found; skipping option summary history",
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
    use crate::sync::client::grpc_manager::{GrpcServerMessageStream, MockGrpcChannelManagerTrait};
    use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxOptionSummaryPo};
    use crate::sync::client::repository::okx::MockClientOkxRepositoryTrait;
    use crate::sync::models::grpc_sync::{HistoryEntry, OkxInstrument, OkxOptionSummary, ServerMessage, history_entry::Payload, instrument};

    fn empty_stream() -> GrpcServerMessageStream {
        Box::pin(futures::stream::empty())
    }

    fn local_instrument(server_id: u64, id: u64) -> LocalOkxInstrumentPo {
        LocalOkxInstrumentPo {
            id,
            server_id,
            inst_identify: "BTC-USDT".to_string(),
            inst_type: "SPOT".to_string(),
            inst_family: None,
            base_ccy: "BTC".to_string(),
            quote_ccy: Some("USDT".to_string()),
            settle_ccy: None,
            list_time: Some(60_000),
            exp_time: None,
            tick_sz: None,
            lot_sz: None,
            min_sz: None,
            alias: None,
            state: Some("live".to_string()),
            inst_id_code: None,
            inst_category: None,
        }
    }

    fn instrument_cache(cached: Option<(u64, u64)>) -> InstrumentCache {
        Arc::new(RwLock::new(
            cached
                .map(|(server_id, id)| HashMap::from([(server_id, local_instrument(server_id, id))]))
                .unwrap_or_default(),
        ))
    }

    fn history_service(
        repository: ClientOkxRepository,
        history_batch_insert: PostgresqlBatchInsert<LocalOkxKlinePo>,
        grpc_manager: GrpcChannelManager,
        binary_search: BinarySearchDS,
        inst_cache: InstrumentCache,
    ) -> OkxKlineClientHistoryService {
        OkxKlineClientHistoryService {
            history_batch_insert,
            repository,
            interval: HistoryInterval::OneMinute,
            grpc_manager,
            binary_search,
            inst_cache,
        }
    }

    fn option_summary_history_service(
        repository: ClientOkxRepository,
        history_batch_insert: PostgresqlBatchInsert<LocalOkxOptionSummaryPo>,
        grpc_manager: GrpcChannelManager,
        inst_cache: InstrumentCache,
    ) -> OkxOptionSummaryClientHistoryService {
        OkxOptionSummaryClientHistoryService {
            history_batch_insert,
            repository,
            grpc_manager,
            inst_cache,
        }
    }

    fn server_instrument(server_id: u64) -> Instrument {
        Instrument {
            instrument_type: InstrumentType::Okx as i32,
            payload: Some(instrument::Payload::Okx(OkxInstrument {
                server_id,
                inst_id: "BTC-USDT".to_string(),
                inst_type: "SPOT".to_string(),
                inst_family: String::new(),
                base_ccy: "BTC".to_string(),
                quote_ccy: "USDT".to_string(),
                settle_ccy: String::new(),
                list_time: 60_000,
                exp_time: 0,
                tick_sz: 0.0,
                lot_sz: 0.0,
                min_sz: 0.0,
                alias: String::new(),
                state: "live".to_string(),
                inst_id_code: String::new(),
                inst_category: String::new(),
            })),
        }
    }

    fn option_summary_entry(server_id: u64) -> HistoryEntry {
        HistoryEntry {
            history_type: HistoryType::OkxOptionSummary as i32,
            instrument_type: InstrumentType::Okx as i32,
            inst_id: server_id,
            payload: Some(Payload::OkxOptionSummary(OkxOptionSummary {
                id: 1,
                inst_identify: "BTC-USD-260101-100000-C".to_string(),
                inst_type: "OPTION".to_string(),
                uly: "BTC-USD".to_string(),
                acquire_ts: 120_000,
                server_ts: 121_000,
                ask_vol: Some(0.5),
                bid_vol: Some(0.4),
                delta: Some(0.6),
                delta_bs: None,
                fwd_px: Some(100_000.0),
                gamma: Some(0.01),
                gamma_bs: None,
                lever: Some(2.0),
                mark_vol: Some(0.7),
                real_vol: None,
                vol_lv: Some(0.8),
                theta: Some(-0.02),
                theta_bs: None,
                vega: Some(0.03),
                vega_bs: None,
            })),
        }
    }

    fn mock_binary_search() -> BinarySearchDS {
        Arc::new(MockBinarySearchDSTrait::new())
    }

    // 测试目的：验证缓存缺少的 OKX instrument 会被创建并写入共享缓存。
    // 准备数据/过程：缓存为空，mock repository 创建时返回本地 ID 7，并提交合法 OKX instrument。
    // 预期结果：repository 收到正确映射的 PO，缓存保存新 instrument 及本地 ID 7。
    #[tokio::test]
    async fn reconcile_instrument_creates_missing_instrument_and_caches_it() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_create_instruments()
            .withf(|po| po.server_id == 42 && po.inst_identify == "BTC-USDT" && po.inst_type == "SPOT" && po.list_time == Some(60_000))
            .times(1)
            .returning(|_| Ok(7));
        repository.expect_update_instrument().times(0);
        let cache = instrument_cache(None);
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: cache.clone(),
        };

        service.reconcile_instrument(server_instrument(42)).await.unwrap();

        let cached = cache.read().await.get(&42).cloned().unwrap();
        assert_eq!(cached.id, 7);
        assert_eq!(cached.inst_identify, "BTC-USDT");
    }

    // 测试目的：验证新建的非 live OKX instrument 不会进入共享缓存。
    // 准备数据/过程：提交 state 为 suspend 的 instrument，并 mock repository 创建成功。
    // 预期结果：instrument 成功写入 repository，但 server ID 42 不在缓存中。
    #[tokio::test]
    async fn reconcile_instrument_does_not_cache_non_live_instrument() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_create_instruments()
            .withf(|po| po.server_id == 42 && po.state.as_deref() == Some("suspend"))
            .times(1)
            .returning(|_| Ok(7));
        repository.expect_update_instrument().times(0);
        let cache = instrument_cache(None);
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: cache.clone(),
        };
        let mut instrument = server_instrument(42);
        if let Some(instrument::Payload::Okx(okx_instrument)) = instrument.payload.as_mut() {
            okx_instrument.state = "suspend".to_string();
        }

        service.reconcile_instrument(instrument).await.unwrap();

        assert!(!cache.read().await.contains_key(&42));
    }

    // 测试目的：验证 OKX instrument 字段未变化时跳过 repository 写入。
    // 准备数据/过程：缓存中预置与服务端 instrument 一致的 PO，并禁止 repository 查找、创建和更新。
    // 预期结果：对账成功，缓存中的本地 ID 和字段保持不变。
    #[tokio::test]
    async fn reconcile_instrument_skips_write_when_cached_instrument_matches() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository.expect_create_instruments().times(0);
        repository.expect_update_instrument().times(0);
        let cache = instrument_cache(Some((42, 7)));
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: cache.clone(),
        };

        service.reconcile_instrument(server_instrument(42)).await.unwrap();

        assert_eq!(cache.read().await.get(&42).map(|instrument| instrument.id), Some(7));
    }

    // 测试目的：验证 OKX instrument 变化时保留本地 ID、更新数据库并刷新缓存。
    // 准备数据/过程：缓存 instrument 的 inst_type 为 SWAP，服务端值为 SPOT，并校验 repository 更新参数。
    // 预期结果：update 被调用一次，缓存更新为服务端字段且仍使用本地 ID 7。
    #[tokio::test]
    async fn reconcile_instrument_updates_changed_instrument() {
        let mut cached = local_instrument(42, 7);
        cached.inst_type = "SWAP".to_string();
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_update_instrument()
            .withf(|po| po.id == 7 && po.server_id == 42 && po.inst_type == "SPOT")
            .times(1)
            .returning(|_| Ok(()));
        repository.expect_create_instruments().times(0);
        let cache = Arc::new(RwLock::new(HashMap::from([(42, cached)])));
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: cache.clone(),
        };

        service.reconcile_instrument(server_instrument(42)).await.unwrap();

        let cached = cache.read().await.get(&42).cloned().unwrap();
        assert_eq!(cached.id, 7);
        assert_eq!(cached.inst_type, "SPOT");
    }

    // 测试目的：验证缓存中的 OKX instrument 变为非 live 状态后会从共享缓存移除。
    // 准备数据/过程：预置 live instrument，并提交 state 为 suspend 的服务端 instrument。
    // 预期结果：repository 更新一次，且 server ID 42 从缓存中移除。
    #[tokio::test]
    async fn reconcile_instrument_removes_non_live_instrument_from_cache() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository
            .expect_update_instrument()
            .withf(|po| po.id == 7 && po.server_id == 42 && po.state.as_deref() == Some("suspend"))
            .times(1)
            .returning(|_| Ok(()));
        repository.expect_create_instruments().times(0);
        let cache = instrument_cache(Some((42, 7)));
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: cache.clone(),
        };
        let mut instrument = server_instrument(42);
        if let Some(instrument::Payload::Okx(okx_instrument)) = instrument.payload.as_mut() {
            okx_instrument.state = "suspend".to_string();
        }

        service.reconcile_instrument(instrument).await.unwrap();

        assert!(!cache.read().await.contains_key(&42));
    }

    // 测试目的：验证 OKX 对账拒绝其他交易所或缺失 payload 的 instrument。
    // 准备数据/过程：分别提交 exchange 类型错误和 OKX payload 为空的 instrument，repository 不配置调用。
    // 预期结果：两个请求均返回校验错误，不读写本地 instrument。
    #[tokio::test]
    async fn reconcile_instrument_rejects_invalid_instrument() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_find_instrument_by_server_id().times(0);
        repository.expect_create_instruments().times(0);
        repository.expect_update_instrument().times(0);
        let service = OkxHistoryClientService {
            history_services: HashMap::new(),
            repository: Arc::new(repository),
            inst_cache: instrument_cache(None),
        };
        let mut wrong_exchange = server_instrument(42);
        wrong_exchange.instrument_type = InstrumentType::Binance as i32;
        let mut missing_payload = server_instrument(42);
        missing_payload.payload = None;

        let exchange_error = service.reconcile_instrument(wrong_exchange).await.unwrap_err();
        let payload_error = service.reconcile_instrument(missing_payload).await.unwrap_err();

        assert!(exchange_error.to_string().contains("exchange is not OKX"));
        assert!(payload_error.to_string().contains("payload is not OKX"));
    }

    // 测试目的：验证初始同步从本地最新 Kline 时间戳之后请求服务端数据。
    // 准备数据/过程：mock 最新时间戳为 120_000，传入 OKX instrument 和结束时间 199_000，再执行 initial_data。
    // 预期结果：SyncRequest 使用 server ID 42、起始时间 120_000、对齐后的结束时间 180_000 和 OkxKline 类型。
    #[tokio::test]
    async fn initial_data_requests_klines_after_the_local_latest_timestamp() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_latest_kline_timestamp_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(Some(120_000)));
        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 120_000
                    && request.end_ms == 180_000
                    && request.history_type == HistoryType::OkxKline as i32
            })
            .returning(|_| Ok(empty_stream()));

        let service = history_service(
            Arc::new(repository),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxKlinePo>::new()),
            Arc::new(grpc_manager),
            mock_binary_search(),
            instrument_cache(Some((42, 7))),
        );

        service.initial_data(server_instrument(42), 199_000).await.unwrap();
    }

    // 测试目的：验证本地不存在服务端 instrument 时会先创建 instrument，再初始化其 Kline 历史。
    // 准备数据/过程：mock 按 server ID 查询无记录、创建操作返回本地 ID 7，并设置无最新 Kline。
    // 预期结果：创建的 PO 正确映射服务端字段，随后使用本地 ID 对应 instrument 的起始时间请求历史数据。
    #[tokio::test]
    async fn initial_data_creates_missing_instrument_before_backfilling() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_instrument_by_server_id()
            .withf(|server_id| *server_id == 42)
            .times(1)
            .returning(|_| Ok(None));
        repository
            .expect_create_instruments()
            .withf(|po| {
                po.id > 0
                    && po.server_id == 42
                    && po.inst_identify == "BTC-USDT"
                    && po.inst_type == "SPOT"
                    && po.base_ccy == "BTC"
                    && po.quote_ccy.as_deref() == Some("USDT")
                    && po.list_time == Some(60_000)
            })
            .times(1)
            .returning(|_| Ok(7));
        repository
            .expect_find_latest_kline_timestamp_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(None));
        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 59_999
                    && request.end_ms == 180_000
                    && request.history_type == HistoryType::OkxKline as i32
            })
            .returning(|_| Ok(empty_stream()));

        let service = history_service(
            Arc::new(repository),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxKlinePo>::new()),
            Arc::new(grpc_manager),
            mock_binary_search(),
            instrument_cache(None),
        );

        service.initial_data(server_instrument(42), 199_000).await.unwrap();
    }

    // 测试目的：验证订阅收到的 OKX Kline 使用服务端 ID 映射到本地 instrument ID 后落库。
    // 准备数据/过程：预置 server ID 42 到本地 ID 7 的映射，提交一条合法的 OKX Kline history entry。
    // 预期结果：batch insert 被调用一次，并收到本地 instrument ID 7。
    #[tokio::test]
    async fn insert_history_uses_cached_local_instrument_id() {
        let repository = MockClientOkxRepositoryTrait::new();
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalOkxKlinePo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| entry.inst_id == 42 && *local_inst_id == 7 && matches!(entry.payload.as_ref(), Some(Payload::OkxKline(_))))
            .times(1)
            .returning(|_, _| ());

        let service = history_service(
            Arc::new(repository),
            Arc::new(batch_insert),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            mock_binary_search(),
            instrument_cache(Some((42, 7))),
        );
        let entry = HistoryEntry {
            history_type: HistoryType::OkxKline as i32,
            instrument_type: InstrumentType::Okx as i32,
            inst_id: 42,
            payload: Some(Payload::OkxKline(crate::sync::models::grpc_sync::OkxKline {
                id: 1,
                ts: 120_000,
                open: 1.0,
                high: 2.0,
                low: 0.5,
                close: 1.5,
                vol: 3.0,
                vol_ccy: 4.0,
                vol_ccy_quote: 5.0,
                confirm: 1,
            })),
        };

        service.insert_history(entry).await.unwrap();
    }

    // 测试目的：验证 history type 和 instrument type 正确但 payload variant 错误时会被拒绝。
    // 准备数据/过程：构造标记为 OKX Kline、实际携带 Polymarket payload 的 HistoryEntry 并调用 insert_history。
    // 预期结果：insert_history 返回错误，batch insert 不接收该记录。
    #[tokio::test]
    async fn insert_history_rejects_non_okx_kline_payload() {
        let service = history_service(
            Arc::new(MockClientOkxRepositoryTrait::new()),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxKlinePo>::new()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            mock_binary_search(),
            instrument_cache(None),
        );
        let entry = HistoryEntry {
            history_type: HistoryType::OkxKline as i32,
            instrument_type: InstrumentType::Okx as i32,
            inst_id: 42,
            payload: Some(Payload::PolymarketHistory(crate::sync::models::grpc_sync::PolyMarketHistory {
                timestamp: 120_000,
                price: 1.0,
            })),
        };

        assert!(service.insert_history(entry).await.is_err());
    }

    // 测试目的：验证数据完整性检查发现在线 instrument 缺少 Kline 时会请求服务端回补缺口。
    // 准备数据/过程：mock 一个 live instrument 和从 60_000 到 180_000 全部缺失的数据区间，执行对账。
    // 预期结果：服务端收到 instrument ID 42、缺口请求范围及 OkxKline 类型正确的 SyncRequest，对账成功。
    #[tokio::test]
    async fn reconcile_backfills_missing_ranges_for_live_instruments() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_list_all_instrument().returning(|| Ok(vec![local_instrument(42, 7)]));
        let mut binary_search = MockBinarySearchDSTrait::new();
        binary_search
            .expect_count_distinct_between()
            .withf(|server_id, start, end| server_id == "42" && *start == 60_000 && *end == 180_000)
            .returning(|_, _, _| Ok(0));
        binary_search.expect_table_name().returning(|| "okx_kline_history".to_string());
        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 59_999
                    && request.end_ms == 180_000
                    && request.history_type == HistoryType::OkxKline as i32
            })
            .returning(|_| Ok(empty_stream()));

        let service = history_service(
            Arc::new(repository),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxKlinePo>::new()),
            Arc::new(grpc_manager),
            Arc::new(binary_search),
            instrument_cache(Some((42, 7))),
        );

        service.reconcile_history_data(239_000).await.unwrap();
    }

    // 测试目的：验证 Option Summary 初始化从本地最新快照之后增量请求服务端历史。
    // 准备数据/过程：预置 server ID 到本地 ID 映射及最新 acquire_ts，并让服务端返回一条 Option Summary。
    // 预期结果：SyncRequest 使用 OkxOptionSummary 类型和 start_ms + 1 的排他起点，历史落库使用本地 instrument ID。
    #[tokio::test]
    async fn option_summary_initial_data_backfills_after_latest_timestamp() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_latest_option_summary_timestamp_by_server_id()
            .withf(|server_id| *server_id == 42)
            .times(1)
            .returning(|_| Ok(Some(120_000)));

        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalOkxOptionSummaryPo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.inst_id == 42 && *local_inst_id == 7 && matches!(entry.payload.as_ref(), Some(Payload::OkxOptionSummary(_)))
            })
            .times(1)
            .returning(|_, _| ());

        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 120_001
                    && request.end_ms == 200_000
                    && request.history_type == HistoryType::OkxOptionSummary as i32
            })
            .returning(|_| {
                Ok(Box::pin(futures::stream::iter(vec![Ok(ServerMessage {
                    history: vec![option_summary_entry(42)],
                    server_timestamp: 200_000,
                })])))
            });

        let service = option_summary_history_service(
            Arc::new(repository),
            Arc::new(batch_insert),
            Arc::new(grpc_manager),
            instrument_cache(Some((42, 7))),
        );

        service.initial_data(server_instrument(42), 200_000).await.unwrap();
    }

    #[tokio::test]
    async fn option_summary_initial_data_creates_missing_instrument() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository
            .expect_find_instrument_by_server_id()
            .withf(|server_id| *server_id == 42)
            .times(1)
            .returning(|_| Ok(None));
        repository
            .expect_create_instruments()
            .withf(|po| po.server_id == 42 && po.inst_identify == "BTC-USDT" && po.list_time == Some(60_000))
            .times(1)
            .returning(|_| Ok(7));
        repository
            .expect_find_latest_option_summary_timestamp_by_server_id()
            .withf(|server_id| *server_id == 42)
            .times(1)
            .returning(|_| Ok(None));

        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 60_001
                    && request.end_ms == 200_000
                    && request.history_type == HistoryType::OkxOptionSummary as i32
            })
            .returning(|_| Ok(empty_stream()));

        let inst_cache = instrument_cache(None);
        let service = option_summary_history_service(
            Arc::new(repository),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxOptionSummaryPo>::new()),
            Arc::new(grpc_manager),
            inst_cache.clone(),
        );

        service.initial_data(server_instrument(42), 200_000).await.unwrap();
        assert_eq!(inst_cache.read().await.get(&42).map(|instrument| instrument.id), Some(7));
    }

    // 测试目的：验证实时 Option Summary 使用 server ID 映射成本地 instrument ID 后落库。
    // 准备数据/过程：预置 server ID 42 到本地 ID 7 的缓存映射并插入合法的 Option Summary history entry。
    // 预期结果：batch insert 收到合法 payload 和本地 ID 7。
    #[tokio::test]
    async fn option_summary_insert_history_uses_cached_local_instrument_id() {
        let mut batch_insert = MockPostgresqlBatchInsertTrait::<LocalOkxOptionSummaryPo>::new();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                entry.inst_id == 42 && *local_inst_id == 7 && matches!(entry.payload.as_ref(), Some(Payload::OkxOptionSummary(_)))
            })
            .times(1)
            .returning(|_, _| ());

        let service = option_summary_history_service(
            Arc::new(MockClientOkxRepositoryTrait::new()),
            Arc::new(batch_insert),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            instrument_cache(Some((42, 7))),
        );

        service.insert_history(option_summary_entry(42)).await.unwrap();
    }

    #[tokio::test]
    async fn option_summary_insert_history_rejects_invalid_payload() {
        let service = option_summary_history_service(
            Arc::new(MockClientOkxRepositoryTrait::new()),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxOptionSummaryPo>::new()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            instrument_cache(Some((42, 7))),
        );
        let mut entry = option_summary_entry(42);
        entry.history_type = HistoryType::OkxKline as i32;

        assert!(service.insert_history(entry).await.is_err());
    }

    #[tokio::test]
    async fn option_summary_reconcile_requests_snapshots_after_the_latest_timestamp() {
        let mut repository = MockClientOkxRepositoryTrait::new();
        repository.expect_list_all_instrument().returning(|| Ok(vec![local_instrument(42, 7)]));
        repository
            .expect_list_option_summary_timestamps()
            .returning(|| Ok(HashMap::from([(42, 120_000)])));

        let mut grpc_manager = MockGrpcChannelManagerTrait::new();
        grpc_manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 120_001
                    && request.end_ms == 200_000
                    && request.history_type == HistoryType::OkxOptionSummary as i32
            })
            .returning(|_| Ok(empty_stream()));

        let service = option_summary_history_service(
            Arc::new(repository),
            Arc::new(MockPostgresqlBatchInsertTrait::<LocalOkxOptionSummaryPo>::new()),
            Arc::new(grpc_manager),
            instrument_cache(Some((42, 7))),
        );

        service.reconcile_history_data(200_000).await.unwrap();
    }
}

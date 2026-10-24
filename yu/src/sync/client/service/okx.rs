use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_okx_kline_batch_insert;
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxKlinePo};
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
use yue::tools::get_snow_flake_id_u64;

type ServerIdToLocalId = Arc<RwLock<HashMap<u64, u64>>>;

pub struct OkxHistoryClientService {
    history_services: HashMap<HistoryType, ClientHistoryService>,
}

impl OkxHistoryClientService {
    pub fn for_production() -> Result<ClientInstrumentService, YuError> {
        let pool = get_sync_client_pg_pool_sync()?;
        let repository = ClientOkxRepositoryImpl::from_pool(pool.clone());
        let id_maps = block_on(repository.server_id_instrument_dictionary())?
            .into_iter()
            .map(|(server_id, instrument)| (server_id, instrument.id))
            .collect();
        let history_service = OkxKlineClientHistoryService::for_production(repository, pool, Arc::new(RwLock::new(id_maps)));
        let history_services = HashMap::from([(HistoryType::OkxKline, history_service)]);
        Ok(Arc::new(Self { history_services }))
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
    id_maps: ServerIdToLocalId,
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

    fn local_instrument_from_server(instrument: OkxInstrument) -> LocalOkxInstrumentPo {
        LocalOkxInstrumentPo {
            id: get_snow_flake_id_u64(),
            server_id: instrument.server_id,
            inst_identify: instrument.inst_id,
            inst_type: instrument.inst_type,
            inst_family: (!instrument.inst_family.is_empty()).then_some(instrument.inst_family),
            base_ccy: instrument.base_ccy,
            quote_ccy: (!instrument.quote_ccy.is_empty()).then_some(instrument.quote_ccy),
            settle_ccy: (!instrument.settle_ccy.is_empty()).then_some(instrument.settle_ccy),
            list_time: (instrument.list_time > 0).then_some(instrument.list_time),
            exp_time: (instrument.exp_time > 0).then_some(instrument.exp_time),
            tick_sz: (instrument.tick_sz != 0.0).then_some(instrument.tick_sz),
            lot_sz: (instrument.lot_sz != 0.0).then_some(instrument.lot_sz),
            min_sz: (instrument.min_sz != 0.0).then_some(instrument.min_sz),
            alias: (!instrument.alias.is_empty()).then_some(instrument.alias),
            state: (!instrument.state.is_empty()).then_some(instrument.state),
            inst_id_code: (!instrument.inst_id_code.is_empty()).then_some(instrument.inst_id_code),
            inst_category: (!instrument.inst_category.is_empty()).then_some(instrument.inst_category),
        }
    }

    fn for_production(repository: ClientOkxRepository, pool: sqlx::PgPool, id_maps: ServerIdToLocalId) -> ClientHistoryService {
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
            id_maps,
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

        let cached_local_inst_id = { self.id_maps.read().await.get(&okx_instrument.server_id).copied() };
        let local_inst_id = match cached_local_inst_id {
            Some(local_inst_id) => local_inst_id,
            None => {
                let local_inst_id = match self.repository.find_instrument_id_by_server_id(okx_instrument.server_id).await? {
                    Some(local_inst_id) => local_inst_id,
                    None => {
                        self.repository
                            .create_instruments(Self::local_instrument_from_server(okx_instrument.clone()))
                            .await?
                    }
                };
                self.id_maps.write().await.insert(okx_instrument.server_id, local_inst_id);
                local_inst_id
            }
        };

        let latest_timestamp = self.repository.find_latest_kline_timestamp_by_server_id(okx_instrument.server_id).await?;
        let start_ms = match latest_timestamp {
            Some(timestamp) => timestamp,
            None if okx_instrument.list_time > 0 => self.first_candle_timestamp(okx_instrument.list_time).saturating_sub(1),
            None => return Err(YuError::new("OKX instrument listing time is unavailable")),
        };
        let end_ms = self.interval.get_close_unix_ms(end_ms);
        self.backfill_history(okx_instrument.server_id, local_inst_id, start_ms, end_ms).await
    }

    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let end_timestamp = self.interval.get_close_unix_ms(end_ms);
        let instruments = self.repository.list_all_instrument().await?;

        for instrument in instruments {
            if !instrument.state.as_deref().is_some_and(|state| state.eq_ignore_ascii_case("live")) {
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
            )?;
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

        let local_inst_id = match self.id_maps.read().await.get(&history_entry.inst_id).copied() {
            Some(local_inst_id) => local_inst_id,
            None => {
                let Some(local_inst_id) = self.repository.find_instrument_id_by_server_id(history_entry.inst_id).await? else {
                    warn!("local OKX instrument {} not found; skipping history", history_entry.inst_id);
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
    use crate::sync::client::grpc_manager::{GrpcServerMessageStream, MockGrpcChannelManagerTrait};
    use crate::sync::client::po::okx::LocalOkxInstrumentPo;
    use crate::sync::client::repository::okx::MockClientOkxRepositoryTrait;
    use crate::sync::models::grpc_sync::{OkxInstrument, instrument};

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

    fn history_service(
        repository: ClientOkxRepository,
        history_batch_insert: PostgresqlBatchInsert<LocalOkxKlinePo>,
        grpc_manager: GrpcChannelManager,
        binary_search: BinarySearchDS,
        id_maps: ServerIdToLocalId,
    ) -> OkxKlineClientHistoryService {
        OkxKlineClientHistoryService {
            history_batch_insert,
            repository,
            interval: HistoryInterval::OneMinute,
            grpc_manager,
            binary_search,
            id_maps,
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

    fn mock_binary_search() -> BinarySearchDS {
        Arc::new(MockBinarySearchDSTrait::new())
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
            Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
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
            .expect_find_instrument_id_by_server_id()
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
            Arc::new(RwLock::new(HashMap::new())),
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
            Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
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
            Arc::new(RwLock::new(HashMap::new())),
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
            Arc::new(RwLock::new(HashMap::from([(42, 7)]))),
        );

        service.reconcile_living_instrument_with_server(239_000).await.unwrap();
    }
}

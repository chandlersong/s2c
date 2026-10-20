use crate::data_integrity::check::{BinarySearchDS, SyncClientBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::db_consts::ClientsTables;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::repository::polymarket::{ClientPolyMarketRepository, default_client_polymarket_repository};
use crate::sync::client::sync_client_service::{ClientExchangeService, ClientExchangeServiceTrait};
use crate::sync::models::grpc_sync::{ExchangeType, Instrument, InstrumentType, SyncRequest, history_entry::Payload, instrument};
use async_trait::async_trait;
use futures::StreamExt;
use li::tools::time::UnixTimeStamp;
use log::{debug, error};
use std::sync::Arc;
use yue::models::HistoryInterval;
use yue::tools::get_snow_flake_id_u64;

pub struct PolymarketClientExchangeService {
    pm_repo: ClientPolyMarketRepository,
    history_batch_insert: PostgresqlBatchInsert<LocalPolyMarketHistoryPo>,
    grpc_manager: GrpcChannelManager,
    binary_search: BinarySearchDS,
    interval: HistoryInterval,
}
impl PolymarketClientExchangeService {
    fn new(
        pm_repo: ClientPolyMarketRepository,
        history_batch_insert: PostgresqlBatchInsert<LocalPolyMarketHistoryPo>,
        grpc_manager: GrpcChannelManager,
        binary_search: BinarySearchDS,
        interval: HistoryInterval,
    ) -> Self {
        Self {
            pm_repo,
            history_batch_insert,
            grpc_manager,
            binary_search,
            interval,
        }
    }

    pub fn for_production() -> Result<ClientExchangeService, YuError> {
        let pm_repo = default_client_polymarket_repository();
        let history_batch_insert = get_polymarket_price_batch_insert();
        let pg_pool = get_sync_client_pg_pool_sync()?;
        let pm_binary_search_ds = SyncClientBinarySearchDataImpl::new(
            pg_pool,
            ClientsTables::PolymarketPriceHistory.table_name(),
            ClientsTables::PolyMarketInstruments.table_name(),
            "timestamp",
        );
        Ok(Arc::new(Self::new(
            pm_repo,
            history_batch_insert,
            get_grpc_manager_from_config(),
            pm_binary_search_ds,
            HistoryInterval::OneHour,
        )))
    }

    async fn backfill_history(&self, server_inst_id: u64, local_inst_id: u64, start_timestamp: u64, end_timestamp: u64) -> Result<(), YuError> {
        if start_timestamp >= end_timestamp {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: server_inst_id,
                start_ms: start_timestamp.saturating_add(1),
                end_ms: end_timestamp,
                instrument_type: InstrumentType::PolymarketToken as i32,
            })
            .await?;

        while let Some(message) = history_stream.next().await {
            let message = message?;
            let batch_timestamp = message.server_timestamp;
            for entry in message.history {
                if entry.instrument_type != InstrumentType::PolymarketToken as i32 {
                    return Err(YuError::new("received non-Polymarket history payload"));
                }

                let Some(Payload::PolymarketHistory(history)) = entry.payload.as_ref() else {
                    return Err(YuError::new("received non-Polymarket history payload"));
                };
                if history.inst_id != server_inst_id {
                    return Err(YuError::new("received Polymarket history for unexpected instrument"));
                }

                self.history_batch_insert.insert_data(entry, local_inst_id).await;
            }
        }

        Ok(())
    }
}
#[async_trait]
impl ClientExchangeServiceTrait for PolymarketClientExchangeService {
    ///
    /// 流程：
    /// 1. 判断instrument是不是polymarket。如果不是，则报错
    /// 2. 判断server_id是否存在，如果存在则新建
    /// 3. 查询数据库中的最大时间戳。如果存在，开始时间取该时间戳。否则，就是inst的list_time
    ///
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        if instrument.exchange != ExchangeType::Polymarket as i32 {
            return Err(YuError::new("instrument exchange is not polymarket"));
        }

        let Some(instrument::Payload::Polymarket(instrument)) = instrument.payload else {
            return Err(YuError::new("instrument payload is not polymarket"));
        };

        let local_instrument = match self.pm_repo.find_instrument_by_server_id(instrument.server_id).await? {
            Some(local_instrument) => local_instrument,
            None => {
                let id = get_snow_flake_id_u64();
                self.pm_repo
                    .create_instruments(LocalPolyMarketInstrumentPo {
                        id,
                        server_id: instrument.server_id,
                        series_id: instrument.series_id,
                        series_slug: instrument.series_slug,
                        event_id: instrument.event_id,
                        event_slug: instrument.event_slug,
                        market_id: instrument.market_id,
                        market_slug: instrument.market_slug,
                        assert_id: instrument.asset_id,
                        assert_slug: instrument.asset_slug,
                        start_ms: instrument.start_ms,
                        end_ms: instrument.end_ms,
                    })
                    .await?;
                self.pm_repo
                    .find_instrument_by_server_id(instrument.server_id)
                    .await?
                    .ok_or_else(|| YuError::new("created Polymarket instrument could not be found"))?
            }
        };

        let latest_timestamp = self.pm_repo.get_lastest_timestamps_by_server_id(instrument.server_id).await?;
        let start_timestamp = if latest_timestamp == 0 { instrument.start_ms } else { latest_timestamp };
        let end_timestamp = end_ms;
        self.backfill_history(instrument.server_id, local_instrument.id, start_timestamp, end_timestamp)
            .await?;
        Ok(())
    }

    ///
    /// 逻辑过程
    /// 1. 通过repo，获取还在运行的instrument的结束时间，小于end_ms
    /// 2. 循环第一步的方法，用二分法找出缺少的代码
    /// 3. 调用backfill。取补全
    ///
    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError> {
        let interval_ms = self.interval.to_milliseconds();
        let end_timestamp = self.interval.get_close_unix_ms(end_ms).saturating_sub(interval_ms);
        let instruments = self.pm_repo.get_live_instruments(end_ms).await?;

        for instrument in instruments {
            let start_timestamp = self.interval.get_close_unix_ms(instrument.start_ms).saturating_add(interval_ms);
            let gaps = binary_search_gap(
                &instrument.server_id.to_string(),
                "SPOT",
                start_timestamp,
                end_timestamp,
                &self.interval,
                self.binary_search.clone(),
            )?;
            debug!("polymarket {} gaps: {}", instrument.server_id, gaps.len());
            for gap in gaps {
                match gap {
                    ValidationGap::MissingData { start_time, end_time, .. } => {
                        self.backfill_history(
                            instrument.server_id,
                            instrument.id,
                            start_time.saturating_sub(1),
                            end_time.saturating_sub(interval_ms),
                        )
                        .await?;
                    }
                    _ => {
                        error!("unexpected Polymarket validation gap: {:?}", gap);
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::check::MockBinarySearchDSTrait;
    use crate::postgresql_db_tables::MockPostgresqlBatchInsertTrait;
    use crate::sync::client::grpc_manager::{GrpcServerMessageStream, MockGrpcChannelManagerTrait};
    use crate::sync::client::repository::polymarket::MockClientPolyMarketRepositoryTrait;
    use crate::sync::models::grpc_sync::{HistoryEntry, PolyMarketHistory, PolymarketInstrument, ServerMessage};
    use li::tools::time::unix_time_now_u64_utc;

    fn empty_history_stream() -> GrpcServerMessageStream {
        Box::pin(futures::stream::empty())
    }

    fn polymarket_instrument(server_id: u64) -> Instrument {
        Instrument {
            payload: Some(instrument::Payload::Polymarket(PolymarketInstrument {
                server_id,
                series_id: "series-id".to_string(),
                series_slug: "series-slug".to_string(),
                event_id: "event-id".to_string(),
                event_slug: "event-slug".to_string(),
                market_id: "market-id".to_string(),
                market_slug: "market-slug".to_string(),
                asset_id: "asset-id".to_string(),
                asset_slug: "asset-slug".to_string(),
                start_ms: 100,
                end_ms: 200,
                latest_timestamp: 200,
            })),
            exchange: ExchangeType::Polymarket as i32,
        }
    }

    fn local_instrument(server_id: u64) -> LocalPolyMarketInstrumentPo {
        LocalPolyMarketInstrumentPo {
            id: 1,
            server_id,
            series_id: "series-id".to_string(),
            series_slug: "series-slug".to_string(),
            event_id: "event-id".to_string(),
            event_slug: "event-slug".to_string(),
            market_id: "market-id".to_string(),
            market_slug: "market-slug".to_string(),
            assert_id: "asset-id".to_string(),
            assert_slug: "asset-slug".to_string(),
            start_ms: 100,
            end_ms: 200,
        }
    }

    fn mock_batch_insert() -> MockPostgresqlBatchInsertTrait<LocalPolyMarketHistoryPo> {
        MockPostgresqlBatchInsertTrait::new()
    }

    fn mock_binary_search_ds() -> BinarySearchDS {
        let mut mock = MockBinarySearchDSTrait::new();
        mock.expect_count_distinct_between().returning(|_, _, _| Ok(0));
        mock.expect_table_name()
            .returning(|| ClientsTables::PolymarketPriceHistory.table_name().to_string());
        mock.expect_symbol_column().returning(|| "server_id".to_string());
        mock.expect_time_column().returning(|| "timestamp".to_string());
        Arc::new(mock)
    }

    // 测试目的：验证本地不存在的 Polymarket instrument 会先写入本地，再从起始时间补取历史。
    // 测试步骤：模拟首次查询不到、创建成功、历史时间戳为空，并检查请求从 start_ms + 1 开始。
    // 预期结果：初始化成功，且按预期的 server_id、起始时间和 instrument 类型发起历史同步。
    #[tokio::test]
    async fn creates_missing_polymarket_instrument() {
        let now = unix_time_now_u64_utc();
        let mut repo = MockClientPolyMarketRepositoryTrait::new();
        repo.expect_find_instrument_by_server_id()
            .withf(|server_id| *server_id == 42)
            .times(2)
            .returning({
                let find_count = Arc::new(std::sync::Mutex::new(0));
                move |_| {
                    let mut find_count = find_count.lock().unwrap();
                    *find_count += 1;
                    if *find_count == 1 { Ok(None) } else { Ok(Some(local_instrument(42))) }
                }
            });
        repo.expect_create_instruments()
            .withf(|po| {
                po.server_id == 42
                    && po.series_id == "series-id"
                    && po.event_id == "event-id"
                    && po.market_id == "market-id"
                    && po.assert_id == "asset-id"
                    && po.start_ms == 100
                    && po.end_ms == 200
            })
            .returning(|_| Ok(()));
        repo.expect_get_lastest_timestamps_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(0));
        let mut manager = MockGrpcChannelManagerTrait::new();
        manager
            .expect_sync_history()
            .withf(|request| request.inst_id == 42 && request.start_ms == 101 && request.instrument_type == InstrumentType::PolymarketToken as i32)
            .returning(|_| Ok(empty_history_stream()));

        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(manager),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );
        service.initial_data(polymarket_instrument(42), now).await.unwrap();
    }

    // 测试目的：验证本地已存在的 Polymarket instrument 不会重复创建。
    // 测试步骤：返回已有 instrument 和本地最新历史时间戳，检查同步请求从该时间戳之后开始。
    // 预期结果：初始化成功，不调用创建操作，并以本地最新时间戳 + 1 作为同步起点。
    #[tokio::test]
    async fn does_not_recreate_existing_instrument() {
        let now = unix_time_now_u64_utc();
        let mut repo = MockClientPolyMarketRepositoryTrait::new();
        repo.expect_find_instrument_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(Some(local_instrument(42))));
        repo.expect_create_instruments().never();
        repo.expect_get_lastest_timestamps_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(150));
        let mut manager = MockGrpcChannelManagerTrait::new();
        manager
            .expect_sync_history()
            .withf(|request| request.inst_id == 42 && request.start_ms == 151 && request.instrument_type == InstrumentType::PolymarketToken as i32)
            .returning(|_| Ok(empty_history_stream()));
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(manager),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );

        service.initial_data(polymarket_instrument(42), now).await.unwrap();
    }

    // 测试目的：独立验证历史同步结果会映射到本地 instrument ID 后交给 batch insert。
    // 测试步骤：直接调用 backfill_history，提供一条服务端历史记录，并检查同步请求和插入参数。
    // 预期结果：请求时间范围为 (start_timestamp, end_timestamp]，且 batch insert 收到映射后的记录。
    #[tokio::test]
    async fn backfill_history_inserts_received_history_using_local_instrument_id() {
        let mut manager = MockGrpcChannelManagerTrait::new();
        manager
            .expect_sync_history()
            .withf(|request| {
                request.inst_id == 42
                    && request.start_ms == 151
                    && request.end_ms == 200
                    && request.instrument_type == InstrumentType::PolymarketToken as i32
            })
            .returning(|_| {
                Ok(Box::pin(futures::stream::iter(vec![Ok(ServerMessage {
                    history: vec![HistoryEntry {
                        instrument_type: InstrumentType::PolymarketToken as i32,
                        payload: Some(Payload::PolymarketHistory(PolyMarketHistory {
                            inst_id: 42,
                            timestamp: 175,
                            price: 0.42,
                        })),
                    }],
                    server_timestamp: 180,
                })])) as GrpcServerMessageStream)
            });
        let mut batch_insert = mock_batch_insert();
        batch_insert
            .expect_insert_data()
            .withf(|entry, local_inst_id| {
                *local_inst_id == 1
                    && entry.instrument_type == InstrumentType::PolymarketToken as i32
                    && matches!(
                        entry.payload.as_ref(),
                        Some(Payload::PolymarketHistory(history))
                            if history.inst_id == 42 && history.timestamp == 175 && history.price == 0.42
                    )
            })
            .times(1)
            .returning(|_, _| ());
        let service = PolymarketClientExchangeService::new(
            Arc::new(MockClientPolyMarketRepositoryTrait::new()),
            Arc::new(batch_insert),
            Arc::new(manager),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );

        service.backfill_history(42, 1, 150, 200).await.unwrap();
    }

    // 测试目的：验证定时校对只检查仍在运行的 instrument，并将缺失区间回补到本地 instrument。
    // 测试步骤：模拟一个仍运行的 instrument 和一个已结束的 instrument，数据源报告运行中的区间全缺失。
    // 预期结果：只为运行中的 instrument 发起历史请求，且请求范围准确覆盖缺失的小时数据。
    #[tokio::test]
    async fn reconciles_gaps_for_living_instruments_only() {
        let interval = HistoryInterval::OneHour;
        let interval_ms = interval.to_milliseconds();
        let end_ms = interval_ms * 6;
        let mut repo = MockClientPolyMarketRepositoryTrait::new();
        repo.expect_get_live_instruments()
            .withf(move |at_ms| *at_ms == end_ms)
            .returning(move |_| {
                Ok(vec![LocalPolyMarketInstrumentPo {
                    end_ms: end_ms + interval_ms,
                    ..local_instrument(42)
                }])
            });

        let mut binary_search = MockBinarySearchDSTrait::new();
        binary_search
            .expect_count_distinct_between()
            .withf(move |server_id, start, end| server_id == "42" && *start == interval_ms && *end == end_ms - interval_ms)
            .returning(|_, _, _| Ok(0));
        binary_search.expect_table_name().returning(|| "polymarket_price_history".to_string());

        let mut manager = MockGrpcChannelManagerTrait::new();
        manager
            .expect_sync_history()
            .withf(move |request| {
                request.inst_id == 42
                    && request.start_ms == interval_ms
                    && request.end_ms == end_ms - 2 * interval_ms
                    && request.instrument_type == InstrumentType::PolymarketToken as i32
            })
            .times(1)
            .returning(|_| Ok(empty_history_stream()));
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(manager),
            Arc::new(binary_search),
            interval,
        );

        service.reconcile_living_instrument_with_server(end_ms).await.unwrap();
    }

    // 测试目的：验证仓库没有返回仍在运行的 instrument 时不会执行缺口查询或历史回补。
    // 测试步骤：mock get_live_instruments 返回空列表。
    // 预期结果：校对正常结束，不调用二分数据源或 gRPC 历史接口。
    #[tokio::test]
    async fn skips_instruments_that_are_not_living() {
        let end_ms = HistoryInterval::OneHour.to_milliseconds() * 6;
        let mut repo = MockClientPolyMarketRepositoryTrait::new();
        repo.expect_get_live_instruments()
            .withf(move |at_ms| *at_ms == end_ms)
            .returning(|_| Ok(vec![]));
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );

        service.reconcile_living_instrument_with_server(end_ms).await.unwrap();
    }

    // 测试目的：确保服务拒绝非 Polymarket 交易所的 instrument。
    // 测试步骤：将 instrument 的 exchange 改为 OKX 后调用初始化。
    // 预期结果：返回错误，且无需访问仓库或发起历史同步。
    #[tokio::test]
    async fn rejects_non_polymarket_instrument() {
        let now = unix_time_now_u64_utc();
        let repo = MockClientPolyMarketRepositoryTrait::new();
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );
        let mut instrument = polymarket_instrument(42);
        instrument.exchange = ExchangeType::Okx as i32;

        assert!(service.initial_data(instrument, now).await.is_err());
    }

    // 测试目的：确保 Polymarket 交易所标记不能搭配缺失的 payload。
    // 测试步骤：保留 Polymarket exchange，将 payload 设置为空后调用初始化。
    // 预期结果：返回错误，不继续执行 instrument 创建或历史同步。
    #[tokio::test]
    async fn rejects_non_polymarket_payload() {
        let now = unix_time_now_u64_utc();
        let repo = MockClientPolyMarketRepositoryTrait::new();
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
            mock_binary_search_ds(),
            HistoryInterval::OneHour,
        );
        let mut instrument = polymarket_instrument(42);
        instrument.payload = None;

        assert!(service.initial_data(instrument, now).await.is_err());
    }
}

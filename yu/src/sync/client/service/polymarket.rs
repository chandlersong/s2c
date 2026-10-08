use crate::errors::YuError;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::grpc_manager::{GrpcChannelManager, get_grpc_manager_from_config};
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::repository::polymarket::{ClientPolyMarketRepository, default_client_polymarket_repository};
use crate::sync::client::sync_client_service::{ClientExchangeService, ClientExchangeServiceTrait};
use crate::sync::models::grpc_sync::{ExchangeType, Instrument, InstrumentType, SyncRequest, instrument, server_message::Payload};
use async_trait::async_trait;
use futures::StreamExt;
use li::tools::time::unix_time_now_u64_utc;
use std::sync::Arc;
use yue::tools::get_snow_flake_id_u64;

pub struct PolymarketClientExchangeService {
    pm_repo: ClientPolyMarketRepository,
    history_batch_insert: PostgresqlBatchInsert<LocalPolyMarketHistoryPo>,
    grpc_manager: GrpcChannelManager,
}
impl PolymarketClientExchangeService {
    fn new(
        pm_repo: ClientPolyMarketRepository,
        history_batch_insert: PostgresqlBatchInsert<LocalPolyMarketHistoryPo>,
        grpc_manager: GrpcChannelManager,
    ) -> Self {
        Self {
            pm_repo,
            history_batch_insert,
            grpc_manager,
        }
    }

    pub fn for_production() -> ClientExchangeService {
        let pm_repo = default_client_polymarket_repository();
        let history_batch_insert = get_polymarket_price_batch_insert();
        Arc::new(Self::new(pm_repo, history_batch_insert, get_grpc_manager_from_config()))
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
    async fn initial_data(&self, instrument: Instrument) -> Result<(), YuError> {
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
        let end_timestamp = unix_time_now_u64_utc();
        if start_timestamp >= end_timestamp {
            return Ok(());
        }

        let mut history_stream = self
            .grpc_manager
            .sync_history(SyncRequest {
                inst_id: instrument.server_id,
                start_ms: start_timestamp.saturating_add(1),
                end_ms: end_timestamp,
                instrument_type: InstrumentType::PolymarketToken as i32,
            })
            .await?;
        while let Some(message) = history_stream.next().await {
            let message = message?;
            match message.payload {
                Some(Payload::PolymarketHistory(history_list)) => {
                    let batch_timestamp = history_list.timestamp;
                    for history in history_list.history_list {
                        if history.inst_id != instrument.server_id {
                            return Err(YuError::new("received Polymarket history for unexpected instrument"));
                        }

                        self.history_batch_insert
                            .insert_data(LocalPolyMarketHistoryPo::from_polymarket_history(
                                history,
                                local_instrument.id,
                                batch_timestamp,
                            ))
                            .await;
                    }
                }
                Some(_) => return Err(YuError::new("received non-Polymarket history payload")),
                None => return Err(YuError::new("received empty history payload")),
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgresql_db_tables::MockPostgresqlBatchInsertTrait;
    use crate::sync::client::grpc_manager::{GrpcServerMessageStream, MockGrpcChannelManagerTrait};
    use crate::sync::client::repository::polymarket::MockClientPolyMarketRepositoryTrait;
    use crate::sync::models::grpc_sync::{PolyMarketHistory, PolyMarketHistoryList, PolymarketInstrument, ServerMessage};

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

    // 测试目的：验证本地不存在的 Polymarket instrument 会先写入本地，再从起始时间补取历史。
    // 测试步骤：模拟首次查询不到、创建成功、历史时间戳为空，并检查请求从 start_ms + 1 开始。
    // 预期结果：初始化成功，且按预期的 server_id、起始时间和 instrument 类型发起历史同步。
    #[tokio::test]
    async fn creates_missing_polymarket_instrument() {
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

        let service = PolymarketClientExchangeService::new(Arc::new(repo), Arc::new(mock_batch_insert()), Arc::new(manager));
        service.initial_data(polymarket_instrument(42)).await.unwrap();
    }

    // 测试目的：验证本地已存在的 Polymarket instrument 不会重复创建。
    // 测试步骤：返回已有 instrument 和本地最新历史时间戳，检查同步请求从该时间戳之后开始。
    // 预期结果：初始化成功，不调用创建操作，并以本地最新时间戳 + 1 作为同步起点。
    #[tokio::test]
    async fn does_not_recreate_existing_instrument() {
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
        let service = PolymarketClientExchangeService::new(Arc::new(repo), Arc::new(mock_batch_insert()), Arc::new(manager));

        service.initial_data(polymarket_instrument(42)).await.unwrap();
    }

    // 测试目的：验证收到的历史数据会映射到本地 instrument ID 后交给 batch insert。
    // 测试步骤：提供本地 instrument ID 与一条服务端历史记录，并校验插入参数。
    // 预期结果：batch insert 恰好收到一条记录，包含本地 ID、历史时间、价格和批次时间。
    #[tokio::test]
    async fn inserts_received_history_using_local_instrument_id() {
        let mut repo = MockClientPolyMarketRepositoryTrait::new();
        repo.expect_find_instrument_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(Some(local_instrument(42))));
        repo.expect_get_lastest_timestamps_by_server_id()
            .withf(|server_id| *server_id == 42)
            .returning(|_| Ok(150));
        let mut manager = MockGrpcChannelManagerTrait::new();
        manager.expect_sync_history().returning(|_| {
            Ok(Box::pin(futures::stream::iter(vec![Ok(ServerMessage {
                payload: Some(Payload::PolymarketHistory(PolyMarketHistoryList {
                    history_list: vec![PolyMarketHistory {
                        inst_id: 42,
                        timestamp: 175,
                        price: 0.42,
                    }],
                    timestamp: 180,
                })),
            })])) as GrpcServerMessageStream)
        });
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new({
                let mut batch_insert = mock_batch_insert();
                batch_insert
                    .expect_insert_data()
                    .withf(|po| po.inst_id == 1 && po.timestamp == 175 && po.price == 0.42 && po.batch_timestamp == 180)
                    .times(1)
                    .returning(|_| ());
                batch_insert
            }),
            Arc::new(manager),
        );

        service.initial_data(polymarket_instrument(42)).await.unwrap();
    }

    // 测试目的：确保服务拒绝非 Polymarket 交易所的 instrument。
    // 测试步骤：将 instrument 的 exchange 改为 OKX 后调用初始化。
    // 预期结果：返回错误，且无需访问仓库或发起历史同步。
    #[tokio::test]
    async fn rejects_non_polymarket_instrument() {
        let repo = MockClientPolyMarketRepositoryTrait::new();
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
        );
        let mut instrument = polymarket_instrument(42);
        instrument.exchange = ExchangeType::Okx as i32;

        assert!(service.initial_data(instrument).await.is_err());
    }

    // 测试目的：确保 Polymarket 交易所标记不能搭配缺失的 payload。
    // 测试步骤：保留 Polymarket exchange，将 payload 设置为空后调用初始化。
    // 预期结果：返回错误，不继续执行 instrument 创建或历史同步。
    #[tokio::test]
    async fn rejects_non_polymarket_payload() {
        let repo = MockClientPolyMarketRepositoryTrait::new();
        let service = PolymarketClientExchangeService::new(
            Arc::new(repo),
            Arc::new(mock_batch_insert()),
            Arc::new(MockGrpcChannelManagerTrait::new()),
        );
        let mut instrument = polymarket_instrument(42);
        instrument.payload = None;

        assert!(service.initial_data(instrument).await.is_err());
    }
}

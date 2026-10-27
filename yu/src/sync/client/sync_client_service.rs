use crate::errors::YuError;
use crate::sync::client::grpc_manager::GrpcChannelManager;
use crate::sync::client::service::binance::BinanceSpotClientInstrumentService;
use crate::sync::client::service::okx::OkxHistoryClientService;
use crate::sync::client::service::polymarket::PolymarketClientInstrumentService;
use crate::sync::models::grpc_sync::{
    ClientBatchWriter, HistoryEntry, HistoryType, Instrument, InstrumentList, InstrumentType, ServerMessage, SubscribeRequest,
};
use async_trait::async_trait;
use futures::StreamExt;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::{error, info, warn};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use yue::tools::get_snow_flake_id_u64;

const INITIAL_DATA_CONCURRENCY: usize = 5;
const INITIAL_DATA_PROGRESS_INTERVAL: Duration = Duration::from_secs(30);

///
/// 该方法中的start和finish其实是钩子，因为在实际中，需要冲数据库中更新一些数据
///
#[async_trait]
pub trait ClientInstrumentServiceTrait: Send + Sync {
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError>;

    fn supported_history_services(&self) -> Result<HashMap<HistoryType, ClientHistoryService>, YuError>;
}

#[async_trait]
#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
pub trait ClientHistoryServiceTrait: Send + Sync {
    ///
    /// 初始化数据。包括两件事情
    /// 1. 更新instrument
    /// 2. 更新history到最新
    ///
    async fn initial_data(&self, instrument: Instrument, end_ms: UnixTimeStamp) -> Result<(), YuError>;

    ///
    /// 校对正在运行的instrument的数据
    ///
    async fn reconcile_living_instrument_with_server(&self, end_ms: UnixTimeStamp) -> Result<(), YuError>;

    async fn insert_history(&self, history_entry: HistoryEntry) -> Result<(), YuError>;
}

pub type ClientInstrumentService = Arc<dyn ClientInstrumentServiceTrait>;
pub type ClientHistoryService = Arc<dyn ClientHistoryServiceTrait>;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ServerMessageProcessorTrait: Send + Sync {
    async fn process_server_message(&self, history_services: &HashMap<HistoryType, ClientHistoryService>, message: ServerMessage);
}

///
/// # 同步Instrument：
/// 同步Instrument本质是两个需求。
/// 1. 同步Instrument对象。
/// 2. 补全Instrument相应的history的数据。
/// 同时，方便处理。
/// Exchange和Instrument之间，原则上是1:n，实际设计的时候是1:1
/// Instrument和history之间，是1:n
///
/// 所以这里分成了多层分别是这里的
/// - SyncClientService：负责总调度
/// - ClientExchangeService： 负责各个交易所的单独处理。
///
/// 至于History。因为exchange无法配置化。所以单独拎出来，写成一个service就毫无意义了。
///
pub struct SyncClientService {
    instrument_services: HashMap<InstrumentType, ClientInstrumentService>,
    history_services: HashMap<HistoryType, ClientHistoryService>,
}

impl SyncClientService {
    pub fn for_production() -> Result<Self, YuError> {
        let mut instrument_services: HashMap<InstrumentType, ClientInstrumentService> = HashMap::new();
        let mut history_services: HashMap<HistoryType, ClientHistoryService> = HashMap::new();

        // 组装polymarket的service
        let polymarket_service = PolymarketClientInstrumentService::for_production()?;
        history_services.extend(polymarket_service.supported_history_services()?);
        // 组装okx的service
        let okx_instruments = OkxHistoryClientService::for_production()?;
        history_services.extend(okx_instruments.supported_history_services()?);
        // 组装Binance Spot和Swap的service
        let binance_instruments = BinanceSpotClientInstrumentService::for_production()?;
        history_services.extend(binance_instruments.supported_history_services()?);

        instrument_services.insert(InstrumentType::Polymarket, polymarket_service.clone());
        instrument_services.insert(InstrumentType::Okx, okx_instruments);
        instrument_services.insert(InstrumentType::Binance, binance_instruments);
        Ok(Self {
            instrument_services,
            history_services,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InstrumentCacheKey {
    instrument_type: i32,
    server_id: u64,
}

impl PartialEq for InstrumentCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_type == other.instrument_type && self.server_id == other.server_id
    }
}

impl Eq for InstrumentCacheKey {}

impl Hash for InstrumentCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.instrument_type.hash(state);
        self.server_id.hash(state);
    }
}

impl SyncClientService {
    pub async fn initial_data(&self, server_inst: InstrumentList) -> Result<(), YuError> {
        // 因为整个流程时间很长，且监听会同时开始。所以要统一结束时间。
        let now = unix_time_now_u64_utc();
        info!("initial_data start");
        let total = server_inst.instruments.len();
        let instrument_services = Arc::new(self.instrument_services.clone());
        let mut instruments = server_inst.instruments.into_iter();
        let mut tasks = JoinSet::new();
        for _ in 0..INITIAL_DATA_CONCURRENCY {
            if let Some(instrument) = instruments.next() {
                tasks.spawn(initialize_instrument(instrument, now, instrument_services.clone()));
            }
        }
        let mut completed = 0;
        let mut progress_interval = tokio::time::interval_at(
            tokio::time::Instant::now() + INITIAL_DATA_PROGRESS_INTERVAL,
            INITIAL_DATA_PROGRESS_INTERVAL,
        );
        while completed < total {
            tokio::select! {
                result = tasks.join_next(), if !tasks.is_empty() => {
                    let Some(result) = result else {
                        continue;
                    };
                    match result {
                        Ok(result) => result?,
                        Err(error) => return Err(YuError::new(&format!("instrument initialization task failed: {error}"))),
                    }
                    completed += 1;
                    if let Some(instrument) = instruments.next() {
                        tasks.spawn(initialize_instrument(instrument, now, instrument_services.clone()));
                    }
                }
                _ = progress_interval.tick() => {
                    info!("initial_data progress: {}/{} instruments completed", completed, total);
                }
            }
        }
        info!("initial_data done");
        Ok(())
    }

    ///
    /// 主要是本地数据和远端服务器的处理。
    /// 1. instrument的列表应该从本地获取。而不是服务器端。因为是本地的服务器。
    /// 2. 技术支持的前提下，应该是只是校对正在运行的polymarket
    ///
    /// # 流程
    /// 1. 确定结束时间。一般默认取now
    /// 2. 获取exchange_services，然后调用其reconcile_living_instrument_with_server。
    /// 3. 如果错误，打印日志，然后跳过。
    ///
    pub async fn reconcile_living_instrument_with_server(&self) -> Result<(), YuError> {
        let now = unix_time_now_u64_utc();
        for (history_type, history_history) in &self.history_services {
            let exchange_name = history_type.readable_name();
            if let Err(e) = history_history.reconcile_living_instrument_with_server(now).await {
                error!("error reconciling exchange {},error:{}", exchange_name, e);
            }
        }
        Ok(())
    }

    ///
    /// # 主要功能
    /// 通过订阅SyncInterfaceClient的SubscribeLatest，获得最新的历史消息，并且存入数据库。
    ///
    /// # 基础步骤
    /// 1. 通过grpc_manager,开始订阅
    /// 2. 获得stream。发过来的ServerMessage，根据history的instrument_type来选择BatchInsert，然后存入。
    ///
    /// # 内部维护和数据说明
    /// 1. ServerMessage.inst_id为本地Instrument的server_id,需要本地维护内容。
    ///     - 可以通过ClientExchangeService来判断是否存在，然后再维护。如果不存在，则可以通过这个来维护
    /// 2. 需要内部判断，根据InstrumentType来维护的batchInsert接口。
    ///
    pub async fn start_subscribe(&self, grpc_manager: GrpcChannelManager) -> Result<ClientBatchWriter, YuError> {
        let client_id = get_snow_flake_id_u64();
        let mut stream = grpc_manager.subscribe_latest(SubscribeRequest { client_id }).await?;
        let (tx, mut rx) = mpsc::channel::<ServerMessage>(10000);

        let history_services = self.history_services.clone();
        let processor = ProductionServerMessageProcessor;
        tokio::spawn(async move {
            loop {
                loop {
                    tokio::select! {
                        message = stream.next() => {
                            match message {
                                Some(Ok(message)) => processor.process_server_message(&history_services, message).await,
                                Some(Err(e)) => {
                                    error!("error receiving SubscribeLatest message: {}", e);
                                    break;
                                }
                                None => break,
                            }
                        }
                        Some(message) = rx.recv() => {
                            processor.process_server_message(&history_services, message).await;
                        }
                    }
                }

                warn!("SubscribeLatest stream ended; reconnecting");
                grpc_manager.reconnect().await;
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                match grpc_manager.subscribe_latest(SubscribeRequest { client_id }).await {
                    Ok(new_stream) => {
                        info!("subscribe latest success");
                        stream = new_stream
                    }
                    Err(e) => {
                        error!("error reconnecting SubscribeLatest stream: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            }
        });

        Ok(tx)
    }
}

async fn initialize_instrument(
    instrument: Instrument,
    end_ms: UnixTimeStamp,
    instrument_services: Arc<HashMap<InstrumentType, ClientInstrumentService>>,
) -> Result<(), YuError> {
    let instrument_type = InstrumentType::try_from(instrument.instrument_type).map_err(|_| YuError::new("unknown exchange type"))?;
    let instrument_name = instrument_type.readable_name();
    let Some(service) = instrument_services.get(&instrument_type) else {
        error!("{} not found instrument sync service to handle", instrument_name);
        return Ok(());
    };
    if let Err(error) = service.initial_data(instrument, end_ms).await {
        error!("error initializing instrument {},error:{}", instrument_name, error);
    }
    Ok(())
}

struct ProductionServerMessageProcessor;

#[async_trait]
impl ServerMessageProcessorTrait for ProductionServerMessageProcessor {
    async fn process_server_message(&self, history_services: &HashMap<HistoryType, ClientHistoryService>, message: ServerMessage) {
        process_server_message(history_services, message).await;
    }
}

async fn process_server_message(history_services: &HashMap<HistoryType, ClientHistoryService>, message: ServerMessage) {
    for entry in message.history {
        let history_type = match HistoryType::try_from(entry.history_type) {
            Ok(history_type) => history_type,
            Err(e) => {
                warn!("ignoring history entry with unknown history type {}: {}", entry.history_type, e);
                continue;
            }
        };
        let Some(history_service) = history_services.get(&history_type) else {
            warn!("no history service for {:?}; ignoring instrument {}", history_type, entry.inst_id);
            continue;
        };
        if let Err(e) = history_service.insert_history(entry).await {
            error!("failed to insert {:?} history: {}", history_type, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::models::grpc_sync::{PolyMarketHistory, history_entry::Payload};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Barrier;

    struct ConcurrencyTrackingInstrumentService {
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        barrier: Arc<Barrier>,
    }

    #[async_trait]
    impl ClientInstrumentServiceTrait for ConcurrencyTrackingInstrumentService {
        async fn initial_data(&self, _instrument: Instrument, _end_ms: UnixTimeStamp) -> Result<(), YuError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.barrier.wait().await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }

        fn supported_history_services(&self) -> Result<HashMap<HistoryType, ClientHistoryService>, YuError> {
            Ok(HashMap::new())
        }
    }

    // 测试目的：验证实时历史消息会路由到注册了对应 HistoryType 的 service。
    // 准备数据/过程：注册 PolymarketToken mock service，并发送一条该类型的 history entry。
    // 预期结果：mock service 收到一次 server ID 为 42 的记录。
    #[tokio::test]
    async fn routes_history_to_service_registered_for_its_history_type() {
        let mut history_service = MockClientHistoryServiceTrait::new();
        history_service
            .expect_insert_history()
            .withf(|entry| entry.inst_id == 42 && entry.history_type == HistoryType::PolymarketToken as i32)
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let history_service: ClientHistoryService = Arc::new(history_service);
        let services = HashMap::from([(HistoryType::PolymarketToken, history_service)]);
        process_server_message(
            &services,
            ServerMessage {
                history: vec![HistoryEntry {
                    history_type: HistoryType::PolymarketToken as i32,
                    instrument_type: InstrumentType::Polymarket as i32,
                    inst_id: 42,
                    payload: Some(Payload::PolymarketHistory(PolyMarketHistory { timestamp: 100, price: 0.5 })),
                }],
                server_timestamp: 100,
            },
        )
        .await;
    }

    // 测试目的：验证初始 instrument 同步最多并发执行 5 项。
    // 准备数据/过程：构造 10 个 OKX instruments，mock service 在 5 个请求同时到达 barrier 后才继续。
    // 预期结果：同步完整处理 10 项，观测到的最大并发数恰为 5。
    #[tokio::test]
    async fn initial_data_limits_concurrency_to_five() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let instrument_service: ClientInstrumentService = Arc::new(ConcurrencyTrackingInstrumentService {
            active: active.clone(),
            max_active: max_active.clone(),
            barrier: Arc::new(Barrier::new(INITIAL_DATA_CONCURRENCY)),
        });
        let service = SyncClientService {
            instrument_services: HashMap::from([(InstrumentType::Okx, instrument_service)]),
            history_services: HashMap::new(),
        };
        let instrument_list = InstrumentList {
            instruments: (0..10)
                .map(|_| Instrument {
                    instrument_type: InstrumentType::Okx as i32,
                    payload: None,
                })
                .collect(),
            server_timestamp: 0,
        };

        service.initial_data(instrument_list).await.unwrap();

        assert_eq!(max_active.load(Ordering::SeqCst), INITIAL_DATA_CONCURRENCY);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

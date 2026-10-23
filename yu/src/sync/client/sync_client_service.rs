use crate::errors::YuError;
use crate::sync::client::database::{
    get_binance_kline_batch_insert, get_binance_trade_batch_insert, get_okx_kline_batch_insert, get_okx_option_summary_batch_insert,
    get_polymarket_price_batch_insert,
};
use crate::sync::client::grpc_manager::GrpcChannelManager;
use crate::sync::client::service::polymarket::PolymarketClientExchangeService;
use crate::sync::models::grpc_sync::{ClientBatchWriter, HistoryType, Instrument, InstrumentList, InstrumentType, ServerMessage, SubscribeRequest};
use async_trait::async_trait;
use futures::StreamExt;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::{error, info, warn};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::mpsc;
use yue::tools::get_snow_flake_id_u64;

const SUPPORTED_HISTORY_TYPES: [HistoryType; 5] = [
    HistoryType::PolymarketToken,
    HistoryType::OkxKline,
    HistoryType::OkxOptionSummary,
    HistoryType::BinanceSpot,
    HistoryType::BinanceTrade,
];

///
/// 该方法中的start和finish其实是钩子，因为在实际中，需要冲数据库中更新一些数据
///
#[async_trait]
pub trait ClientExchangeServiceTrait: Send + Sync {
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

    ///
    /// 判断对应的server_id是否存在。如果存在，返回该instrument的id。反之，则返回None
    ///
    async fn check_server_id(&self, server_id: u64) -> Result<Option<u64>, YuError>;
}

pub type ClientExchangeService = Arc<dyn ClientExchangeServiceTrait>;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait ServerMessageProcessorTrait: Send + Sync {
    async fn process_server_message(
        &self,
        exchange_services: &HashMap<InstrumentType, ClientExchangeService>,
        local_instrument_ids: &mut HashMap<InstrumentCacheKey, u64>,
        message: ServerMessage,
    );
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
    instrument_services: HashMap<InstrumentType, ClientExchangeService>,
}

impl SyncClientService {
    pub fn for_production() -> Result<Self, YuError> {
        let mut exchange_services = HashMap::new();
        exchange_services.insert(InstrumentType::Polymarket, PolymarketClientExchangeService::for_production()?);
        Ok(Self {
            instrument_services: exchange_services,
        })
    }
}

// key: server_id, value: (start_ms, end_ms)
pub struct InstrumentsDiff {
    pub polymarket_diff: HashMap<u64, (u64, u64)>,
    pub okx_option_kline_diff: HashMap<u64, (u64, u64)>,
    pub okx_option_summary_diff: HashMap<u64, (u64, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinanceTradeSyncRange {
    pub start_ms: u64,
    pub end_ms: u64,
    pub start_trade_id: u64,
    pub end_trade_id: u64,
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
        for inst in server_inst.instruments.into_iter() {
            let exchange_type = InstrumentType::try_from(inst.instrument_type).map_err(|_| YuError::new("unknown exchange type"))?;
            let exchange_name = exchange_type.readable_name();
            let exchange_service = self.instrument_services.get(&exchange_type);
            if exchange_service.is_none() {
                error!("{} not found exchange service to handle", exchange_name);
                continue;
            }
            if let Err(e) = exchange_service.unwrap().initial_data(inst, now).await {
                error!("error initializing exchange {},error:{}", exchange_name, e);
            }
        }
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
        for (exchange_type, exchange_service) in &self.instrument_services {
            let exchange_name = exchange_type.readable_name();
            if let Err(e) = exchange_service.reconcile_living_instrument_with_server(now).await {
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

        let exchange_services = self.instrument_services.clone();
        let processor = ProductionServerMessageProcessor;

        tokio::spawn(async move {
            let mut local_instrument_ids = HashMap::new();
            while let Some(message) = rx.recv().await {
                processor
                    .process_server_message(&exchange_services, &mut local_instrument_ids, message)
                    .await;
            }
        });

        let stream_tx = tx.clone();
        tokio::spawn(async move {
            loop {
                while let Some(message) = stream.next().await {
                    match message {
                        Ok(message) => {
                            if stream_tx.send(message).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            error!("error receiving SubscribeLatest message: {}", e);
                            break;
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

struct ProductionServerMessageProcessor;

#[async_trait]
impl ServerMessageProcessorTrait for ProductionServerMessageProcessor {
    async fn process_server_message(
        &self,
        exchange_services: &HashMap<InstrumentType, ClientExchangeService>,
        local_instrument_ids: &mut HashMap<InstrumentCacheKey, u64>,
        message: ServerMessage,
    ) {
        process_server_message(exchange_services, local_instrument_ids, message).await;
    }
}

async fn process_server_message(
    exchange_services: &HashMap<InstrumentType, ClientExchangeService>,
    local_instrument_ids: &mut HashMap<InstrumentCacheKey, u64>,
    message: ServerMessage,
) {
    for entry in message.history {
        let instrument_type = match InstrumentType::try_from(entry.instrument_type) {
            Ok(instrument_type) => instrument_type,
            Err(e) => {
                warn!("ignoring history entry with unknown instrument type {}: {}", entry.instrument_type, e);
                continue;
            }
        };
        let history_type = match HistoryType::try_from(entry.history_type) {
            Ok(history_type) => history_type,
            Err(e) => {
                warn!("ignoring history entry with unknown history type {}: {}", entry.history_type, e);
                continue;
            }
        };
        if !SUPPORTED_HISTORY_TYPES.contains(&history_type) {
            warn!("ignoring unsupported history type {:?} for instrument {}", history_type, entry.inst_id);
            continue;
        }

        let Some(exchange_service) = exchange_services.get(&instrument_type) else {
            warn!("no exchange service for {:?}; ignoring instrument {}", instrument_type, entry.inst_id);
            continue;
        };
        let cache_key = InstrumentCacheKey {
            instrument_type: entry.instrument_type,
            server_id: entry.inst_id,
        };
        let local_inst_id = match local_instrument_ids.get(&cache_key).copied() {
            Some(local_inst_id) => local_inst_id,
            None => match exchange_service.check_server_id(entry.inst_id).await {
                Ok(Some(local_inst_id)) => {
                    local_instrument_ids.insert(cache_key, local_inst_id);
                    local_inst_id
                }
                Ok(None) => {
                    warn!("local instrument not found for server instrument {}", entry.inst_id);
                    continue;
                }
                Err(e) => {
                    error!("failed to look up local instrument {}: {}", entry.inst_id, e);
                    continue;
                }
            },
        };

        match history_type {
            HistoryType::PolymarketToken => {
                get_polymarket_price_batch_insert().insert_data(entry, local_inst_id).await;
            }
            HistoryType::OkxKline => {
                get_okx_kline_batch_insert().insert_data(entry, local_inst_id).await;
            }
            HistoryType::OkxOptionSummary => {
                get_okx_option_summary_batch_insert().insert_data(entry, local_inst_id).await;
            }
            HistoryType::BinanceSpot => {
                get_binance_kline_batch_insert().insert_data(entry, local_inst_id).await;
            }
            HistoryType::BinanceTrade => {
                get_binance_trade_batch_insert().insert_data(entry, local_inst_id).await;
            }
            HistoryType::InstrumentUnspecified => unreachable!("unsupported history type was filtered above"),
        }
    }
}

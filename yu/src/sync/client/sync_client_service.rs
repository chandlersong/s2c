use crate::errors::YuError;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_okx_kline_batch_insert;
use crate::sync::client::database::get_okx_option_summary_batch_insert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxKlinePo};
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::service::polymarket::PolymarketClientExchangeService;
use crate::sync::models::grpc_sync::{ExchangeType, Instrument, InstrumentList, ServerMessage};
use async_trait::async_trait;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc};
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

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
}

pub type ClientExchangeService = Arc<dyn ClientExchangeServiceTrait>;

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
    pm_instrument_dict: Arc<RwLock<HashMap<u64, LocalPolyMarketInstrumentPo>>>,
    okx_instrument_dict: Arc<RwLock<HashMap<u64, LocalOkxInstrumentPo>>>,

    exchange_services: HashMap<ExchangeType, ClientExchangeService>,
}

impl SyncClientService {
    pub fn for_production() -> Result<Self, YuError> {
        let mut exchange_services = HashMap::new();
        exchange_services.insert(ExchangeType::Polymarket, PolymarketClientExchangeService::for_production()?);
        Ok(Self {
            pm_instrument_dict: Arc::new(Default::default()),
            okx_instrument_dict: Arc::new(Default::default()),
            exchange_services,
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

impl SyncClientService {
    pub async fn initial_data(&self, server_inst: InstrumentList) -> Result<(), YuError> {
        // 因为整个流程时间很长，且监听会同时开始。所以要统一结束时间。
        let now = unix_time_now_u64_utc();
        for (_, inst) in server_inst.instruments.into_iter() {
            let exchange_type = ExchangeType::try_from(inst.exchange).map_err(|_| YuError::new("unknown exchange type"))?;
            let exchange_name = exchange_type.readable_name();
            let exchange_service = self.exchange_services.get(&exchange_type);
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
        for (exchange_type, exchange_service) in &self.exchange_services {
            let exchange_name = exchange_type.readable_name();
            if let Err(e) = exchange_service.reconcile_living_instrument_with_server(now).await {
                error!("error reconciling exchange {},error:{}", exchange_name, e);
            }
        }
        Ok(())
    }

    ///
    /// 监听数据，写入数据库
    ///
    pub async fn start_batch_insert(
        &self,
        polymarket_batch_insert: Option<PostgresqlBatchInsert<LocalPolyMarketHistoryPo>>,
        okx_batch_insert: Option<PostgresqlBatchInsert<LocalOkxKlinePo>>,
    ) -> Result<mpsc::Sender<ServerMessage>, YuError> {
        let (tx, mut rx) = mpsc::channel::<ServerMessage>(10000);

        let do_batch_insert = polymarket_batch_insert.unwrap_or_else(|| get_polymarket_price_batch_insert());

        // build server_id -> LocalPolyMarketInstrumentPo mapping to translate server inst_id to local inst id
        let server_id_map = self.pm_instrument_dict.clone();
        let okx_server_map = self.okx_instrument_dict.clone();
        let do_okx_batch_insert = okx_batch_insert.unwrap_or_else(|| get_okx_kline_batch_insert());
        let do_okx_option_batch_insert = get_okx_option_summary_batch_insert();

        tokio::spawn(async move { loop {} });

        Ok(tx)
    }
}

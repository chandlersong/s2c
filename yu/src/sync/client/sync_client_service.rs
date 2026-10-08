use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_okx_kline_batch_insert;
use crate::sync::client::database::get_okx_option_summary_batch_insert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxKlinePo, LocalOkxOptionSummaryPo};
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::repository::okx::{ClientOkxRepository, ClientOkxRepositoryImpl};
use crate::sync::client::repository::polymarket::{ClientPolyMarketRepository, ClientPolyMarketRepositoryImpl};
use crate::sync::client::service::polymarket::PolymarketClientExchangeService;
use crate::sync::models::grpc_sync::server_message::Payload;
use crate::sync::models::grpc_sync::{ExchangeType, Instrument, InstrumentList, ServerMessage, instrument};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc;
use log::{debug, error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use yue::tools::get_snow_flake_id_u64;

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
    async fn initial_data(&self, instrument: Instrument) -> Result<(), YuError>;
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
/// 所以这里分成了多层分别是这里的SyncClientService，ClientExchangeService
/// 设计中，ClientHistoryService应该每个自己去实现的。而另外两个是可以处理的。
///
pub struct SyncClientService {
    pm_repository: ClientPolyMarketRepository,
    okx_repository: ClientOkxRepository,
    pm_instrument_dict: Arc<RwLock<HashMap<u64, LocalPolyMarketInstrumentPo>>>,
    okx_instrument_dict: Arc<RwLock<HashMap<u64, LocalOkxInstrumentPo>>>,

    exchange_services: HashMap<ExchangeType, ClientExchangeService>,
}

impl Default for SyncClientService {
    fn default() -> Self {
        let mut exchange_services = HashMap::new();
        exchange_services.insert(ExchangeType::Polymarket, PolymarketClientExchangeService::for_production());
        Self {
            pm_repository: ClientPolyMarketRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
            okx_repository: ClientOkxRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
            pm_instrument_dict: Arc::new(Default::default()),
            okx_instrument_dict: Arc::new(Default::default()),
            exchange_services,
        }
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
    ///
    /// # 方法作用
    /// 校准本地数据库  info
    /// 1. 如果有新的asset info。那么就加入本地数据库
    /// 2. 返回一个map，为本地数据库的时间戳，和服务器端不一样的。
    ///
    /// # 步骤
    /// 1. 找出local_assets和server_assets的不同。做以下操纵。
    ///    1. 如果存在于server_assets中，但是不存在于local_assets中，
    ///       - 把server_asset转换成LocalPolyMarketAssetInfoPo，存入数据库
    ///       - 加入返回值，key为assert_id, value为0
    ///    2. 如果存在于local_assets中，但是不存在于server_assets中，跳过，不做任何操作
    ///    3，都存在的话，那么比较local_assets的timestamp和服务器端的timestamp
    ///        - 如果local的timestamp小于server端的timestamp。则加入返回值，key为asset_id,value为local的timestamp
    ///
    pub async fn initial_data(&self, server_inst: InstrumentList) -> Result<InstrumentsDiff, YuError> {
        // fetch local instruments and build a lookup by assert_id -> end_ts
        let now = unix_time_now_u64_utc();
        let mut pm_local_inst: HashMap<u64, u64> = HashMap::new();
        for inst in self.pm_repository.list_all_instrument().await?.into_iter() {
            pm_local_inst.insert(inst.server_id.clone(), inst.end_ms);
        }
        let mut okx_local_inst: HashMap<u64, u64> = HashMap::new();
        for inst in self.okx_repository.list_all_instrument().await?.into_iter() {
            okx_local_inst.insert(inst.server_id.clone(), inst.exp_time.unwrap_or(0));
        }
        info!("local instruments num: {}", pm_local_inst.len());

        let mut polymarket_diff: HashMap<u64, (u64, u64)> = HashMap::new();
        let mut okx_option_diff: HashMap<u64, (u64, u64)> = HashMap::new();
        let mut okx_option_summary_diff: HashMap<u64, (u64, u64)> = HashMap::new();

        // server_inst.instruments: map<string, PolymarketAssertInfo>
        for (_, inst) in server_inst.instruments.into_iter() {
            let exchange_type = ExchangeType::try_from(inst.exchange).map_err(|_| YuError::new("unknown exchange type"))?;
            let exchange_name = exchange_type.readable_name();
            let exchange_service = self.exchange_services.get(&exchange_type).unwrap();
            if let Err(e) = exchange_service.initial_data(inst).await {
                error!("error initializing exchange {},error:{}", exchange_name, e);
            }
        }

        Ok(InstrumentsDiff {
            polymarket_diff,
            okx_option_kline_diff: okx_option_diff,
            okx_option_summary_diff,
        })
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

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(server_msg) = rx.recv() => {
                        if let Some(payload) = server_msg.payload {
                            match payload {
                            Payload::PolymarketHistory(history) => {
                                    let batch_timestamp = history.timestamp;
                                    for h in history.history_list.into_iter() {
                                        // map server inst id to local inst id when available
                                        match server_id_map.read().await.get(&h.inst_id){
                                            None => {
                                                error!("server inst_id {} not found in local mapping, skipping", h.inst_id);
                                            }
                                            Some(po) => {
                                                 let local_inst_id = po.id;
                                                 let po = LocalPolyMarketHistoryPo::from_polymarket_history(h, local_inst_id, batch_timestamp);
                                                 do_batch_insert.insert_data(po).await;
                                            }
                                        }
                                    }
                                }
                            Payload::OkxKlineHistory(kline_list) => {
                                let batch_timestamp = kline_list.timestamp;
                                for k in kline_list.kline_list.into_iter() {
                                    match okx_server_map.read().await.get(&k.inst_id) {
                                        None => {
                                            error!("okx inst_id {} not found in local mapping, skipping", k.inst_id);
                                        }
                                        Some(inst_po) => {
                                            let local_id = inst_po.id;
                                            let po = LocalOkxKlinePo::from_okx_kline(k, local_id, batch_timestamp);
                                            do_okx_batch_insert.insert_data(po).await;
                                        }
                                    }
                                }
                            }
                            Payload::OptionSummaryHistory(summary_list) => {
                                let batch_timestamp = summary_list.timestamp;
                                for s in summary_list.summary_list.into_iter() {
                                    match okx_server_map.read().await.get(&s.inst_id) {
                                        None => {
                                            error!("okx option summary inst_id {} not found in local mapping, skipping", s.inst_id);
                                        }
                                        Some(inst_po) => {
                                            let local_id = inst_po.id;
                                            let po = LocalOkxOptionSummaryPo::from_okx_summary(s, local_id, batch_timestamp);
                                            do_okx_option_batch_insert.insert_data(po).await;
                                        }
                                    }
                                }
                            }
                            Payload::BinanceKlineHistory(_) => {
                               debug!("receive BinanceKlineHistory")
                            }
                            Payload::BinanceTradeHistory(_) => {
                              debug!("receive BinanceTradeHistory")
                            }
                            }
                        }
                    }
                }
            }
        });

        Ok(tx)
    }
}

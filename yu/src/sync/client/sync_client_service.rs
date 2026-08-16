use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_okx_kline_batch_insert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::po::okx::{LocalOkxInstrumentPo, LocalOkxKlinePo};
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::repository::okx::{ClientOkxRepository, ClientOkxRepositoryImpl};
use crate::sync::client::repository::polymarket::{ClientPolyMarketRepository, ClientPolyMarketRepositoryImpl};
use crate::sync::models::grpc_sync::server_message::Payload;
use crate::sync::models::grpc_sync::{InstrumentList, ServerMessage, instrument};
use governor::Jitter;
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock, mpsc};
use tonic::transport::{Channel, Endpoint};
use yue::tools::get_snow_flake_id_u64;

struct ConnectionHolder {
    channel: OnceLock<Channel>,
    reconnect_notify: Notify,   // 通知等待者
    reconnect_mutex: Mutex<()>, // 防止多个进程同时重连
}

pub struct GrpcChannelManager {
    inner: Arc<ConnectionHolder>,
    server_url: String,
}

impl GrpcChannelManager {
    pub fn new(server_url: &str) -> Self {
        Self {
            inner: Arc::new(ConnectionHolder {
                channel: OnceLock::new(),
                reconnect_notify: Notify::new(),
                reconnect_mutex: Mutex::new(()),
            }),
            server_url: server_url.to_string(),
        }
    }

    /// 获取 Channel（会自动初始化或等待）
    pub async fn get_channel(&self) -> Channel {
        // 第一次或断开后
        if let Some(ch) = self.inner.channel.get() {
            return ch.clone();
        }

        self.reconnect().await
    }

    /// 核心：带跨进程锁的重连逻辑
    pub async fn reconnect(&self) -> Channel {
        // 先尝试获取跨进程锁（只有一个进程能真正重连）
        let _guard = match self.inner.reconnect_mutex.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // 其他进程等待通知
                println!("其他进程等待重连完成...");
                self.inner.reconnect_notify.notified().await;
                return self.inner.channel.get().unwrap().clone();
            }
        };

        // ==================== 真正执行重连的进程 ====================
        info!("当前进程正在重建 gRPC 连接...");
        self.connect().await
    }

    pub async fn connect(&self) -> Channel {
        loop {
            match Endpoint::from_shared(self.server_url.clone()) {
                Ok(endpoint) => match endpoint.connect().await {
                    Ok(new_channel) => {
                        let _ = self.inner.channel.set(new_channel.clone());
                        self.inner.reconnect_notify.notify_waiters(); // 通知所有等待者
                        info!("连接到远程成功");
                        return new_channel;
                    }
                    Err(e) => {
                        error!("连接失败: {}, 2秒后重试", e);
                        let jitter = Jitter::up_to(Duration::from_millis(1000));
                        let duration = jitter + Duration::from_millis(1500);
                        tokio::time::sleep(duration).await;
                    }
                },
                Err(e) => {
                    error!("invalid server url: {}, 2秒后重试", e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}

pub struct SyncClientService {
    pm_repository: ClientPolyMarketRepository,
    okx_repository: ClientOkxRepository,
    pm_instrument_dict: Arc<RwLock<HashMap<u64, LocalPolyMarketInstrumentPo>>>,
    okx_instrument_dict: Arc<RwLock<HashMap<u64, LocalOkxInstrumentPo>>>,
}

impl Default for SyncClientService {
    fn default() -> Self {
        Self {
            pm_repository: ClientPolyMarketRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
            okx_repository: ClientOkxRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
            pm_instrument_dict: Arc::new(Default::default()),
            okx_instrument_dict: Arc::new(Default::default()),
        }
    }
}

// key: server_id, value: (start_ms, end_ms)
pub struct InstrumentsDiff {
    pub polymarket_diff: HashMap<u64, (u64, u64)>,
    pub okx_option_diff: HashMap<u64, (u64, u64)>,
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
    pub async fn align_local_instrument(&self, server_inst: InstrumentList) -> Result<InstrumentsDiff, YuError> {
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

        let local_pm_history_latest = self.pm_repository.list_instrument_timestamps().await?;
        let local_okx_history_latest = self.okx_repository.list_instrument_timestamps().await?;

        // server_inst.instruments: map<string, PolymarketAssertInfo>
        for (_, inst) in server_inst.instruments.into_iter() {
            if let Some(payload) = inst.payload {
                match payload {
                    instrument::Payload::Okx(okx_inst) => {
                        // handle okx: insert if missing into local okx_instruments table
                        let server_id = okx_inst.server_id.clone();
                        if !okx_local_inst.contains_key(&server_id) {
                            let po = LocalOkxInstrumentPo {
                                id: get_snow_flake_id_u64(),
                                server_id: okx_inst.server_id,
                                inst_identify: okx_inst.inst_id.clone(),
                                inst_type: okx_inst.inst_type.clone(),
                                inst_family: if okx_inst.inst_family.is_empty() {
                                    None
                                } else {
                                    Some(okx_inst.inst_family.clone())
                                },
                                base_ccy: okx_inst.base_ccy.clone(),
                                quote_ccy: if okx_inst.quote_ccy.is_empty() {
                                    None
                                } else {
                                    Some(okx_inst.quote_ccy.clone())
                                },
                                settle_ccy: if okx_inst.settle_ccy.is_empty() {
                                    None
                                } else {
                                    Some(okx_inst.settle_ccy.clone())
                                },
                                list_time: Some(okx_inst.list_time),
                                exp_time: Some(okx_inst.exp_time),
                                tick_sz: Some(okx_inst.tick_sz),
                                lot_sz: Some(okx_inst.lot_sz),
                                min_sz: Some(okx_inst.min_sz),
                                alias: if okx_inst.alias.is_empty() { None } else { Some(okx_inst.alias.clone()) },
                                state: if okx_inst.state.is_empty() { None } else { Some(okx_inst.state.clone()) },
                                inst_id_code: if okx_inst.inst_id_code.is_empty() {
                                    None
                                } else {
                                    Some(okx_inst.inst_id_code.clone())
                                },
                                inst_category: if okx_inst.inst_category.is_empty() {
                                    None
                                } else {
                                    Some(okx_inst.inst_category.clone())
                                },
                            };
                            self.okx_repository.create_instruments(po).await?;
                            okx_option_diff.insert(server_id, (okx_inst.list_time, now));
                        } else {
                            let local_ts = *local_okx_history_latest.get(&server_id).unwrap_or(&0u64);
                            okx_option_diff.insert(server_id, (local_ts, now));
                        }

                        // we don't add okx entries to the polymarket return map (res) because it expects u64 keys
                        continue;
                    }
                    instrument::Payload::Polymarket(instrument) => {
                        let inst_id = instrument.server_id;
                        if !pm_local_inst.contains_key(&inst_id) {
                            // insert into local db
                            let po = LocalPolyMarketInstrumentPo {
                                id: get_snow_flake_id_u64(),
                                server_id: instrument.server_id,
                                series_id: instrument.series_id.clone(),
                                series_slug: instrument.series_slug.clone(),
                                event_id: instrument.event_id.clone(),
                                event_slug: instrument.event_slug.clone(),
                                market_id: instrument.market_id.clone(),
                                market_slug: instrument.market_slug.clone(),
                                assert_id: instrument.asset_id.clone(),
                                assert_slug: instrument.asset_slug.clone(),
                                start_ms: instrument.start_ms,
                                end_ms: instrument.end_ms,
                            };

                            self.pm_repository.create_instruments(po).await?;
                            polymarket_diff.insert(inst_id, (instrument.start_ms, now));
                        } else {
                            let local_ts = *local_pm_history_latest.get(&inst_id).unwrap_or(&0u64);
                            polymarket_diff.insert(inst_id, (local_ts, now));
                        }
                    }
                }
            }
        }
        {
            let inst_map = self.pm_repository.server_id_instrument_dictionary().await?;
            let mut guard = self.pm_instrument_dict.write().await;
            *guard = inst_map;
        }
        {
            let inst_map = self.okx_repository.server_id_instrument_dictionary().await?;
            let mut guard = self.okx_instrument_dict.write().await;
            *guard = inst_map;
        }
        Ok(InstrumentsDiff {
            polymarket_diff,
            okx_option_diff,
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
                            }
                        }
                    }
                }
            }
        });

        Ok(tx)
    }
}

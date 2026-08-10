use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::po::polymarket::{LocalPolyMarketHistoryPo, LocalPolyMarketInstrumentPo};
use crate::sync::client::repository::polymarket::{ClientPolyMarketRepository, ClientPolyMarketRepositoryImpl};
use crate::sync::models::grpc_sync::server_message::Payload;
use crate::sync::models::grpc_sync::{InstrumentList, ServerMessage, instrument};
use governor::Jitter;
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
    repository: ClientPolyMarketRepository,
    instrument_dict: Arc<RwLock<HashMap<u64, LocalPolyMarketInstrumentPo>>>,
}

impl Default for SyncClientService {
    fn default() -> Self {
        Self {
            repository: ClientPolyMarketRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
            instrument_dict: Arc::new(Default::default()),
        }
    }
}

impl SyncClientService {
    ///
    /// # 方法作用
    /// 校准本地数据库 asset info
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
    pub async fn align_local_instrument(&self, server_inst: InstrumentList) -> Result<HashMap<u64, u64>, YuError> {
        // fetch local instruments and build a lookup by assert_id -> end_ts
        let local_inst = self.repository.list_all_instrument().await?;
        let mut local_map: HashMap<u64, u64> = HashMap::new();
        for inst in local_inst.into_iter() {
            local_map.insert(inst.server_id.clone(), inst.end_ms);
        }
        info!("local instruments num: {}", local_map.len());

        let mut res: HashMap<u64, u64> = HashMap::new();

        let local_history_latest = self.repository.list_instrument_timestamps().await?;

        // server_inst.instruments: map<string, PolymarketAssertInfo>
        for (_, inst) in server_inst.instruments.into_iter() {
            if let Some(payload) = inst.payload {
                match payload {
                    instrument::Payload::Okx(_) => {
                        // skip okx for now
                        continue;
                    }
                    instrument::Payload::Polymarket(instrument) => {
                        let inst_id = instrument.server_id;
                        if !local_map.contains_key(&inst_id) {
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

                            self.repository.create_instruments(po).await?;
                            res.insert(inst_id, instrument.start_ms);
                        } else {
                            let local_ts = *local_history_latest.get(&inst_id).unwrap_or(&0u64);

                            res.insert(inst_id, local_ts + 1);
                        }
                    }
                }
            }
        }
        {
            let inst_map = self.repository.server_id_instrument_dictionary().await?;
            let mut guard = self.instrument_dict.write().await;
            *guard = inst_map;
        }
        Ok(res)
    }

    ///
    /// 监听数据，写入数据库
    ///
    pub async fn start_batch_insert(
        &self,
        batch_insert: Option<PostgresqlBatchInsert<LocalPolyMarketHistoryPo>>,
    ) -> Result<mpsc::Sender<ServerMessage>, YuError> {
        let (tx, mut rx) = mpsc::channel::<ServerMessage>(10000);

        let do_batch_insert = batch_insert.unwrap_or_else(|| get_polymarket_price_batch_insert());

        // build server_id -> LocalPolyMarketInstrumentPo mapping to translate server inst_id to local inst id
        let server_id_map = self.instrument_dict.clone();

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
                            Payload::OkxKlineHistory(_) => {} }
                        }
                    }
                }
            }
        });

        Ok(tx)
    }
}

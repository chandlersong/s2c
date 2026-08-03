use crate::errors::YuError;
use crate::postgresql_db::get_sync_client_pg_pool_sync;
use crate::postgresql_db_tables::PostgresqlBatchInsert;
use crate::sync::client::database::get_polymarket_price_batch_insert;
use crate::sync::client::po::{LocalPolyMarketAssetInfoPo, LocalPolyMarketHistoryPo};
use crate::sync::client::repository::{ClientPolyMarketRepository, ClientPolyMarketRepositoryImpl};
use crate::sync::models::grpc_sync::server_message::Payload;
use crate::sync::models::grpc_sync::{Instrument, ServerMessage};
use governor::Jitter;
use log::{error, info};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, Notify, mpsc};
use tonic::transport::{Channel, Endpoint};

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
}

impl Default for SyncClientService {
    fn default() -> Self {
        Self {
            repository: ClientPolyMarketRepositoryImpl::from_pool(get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool_sync failed")),
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
    pub async fn align_local_assets(&self, server_inst: Instrument) -> Result<HashMap<String, u64>, YuError> {
        let local_assets = self.repository.list_assets_timestamp().await?;
        info!("local assets history num: {}", local_assets.len());

        let mut res: HashMap<String, u64> = HashMap::new();

        // server_assets.assets: map<string, PolymarketAssertInfo>
        // for (_key, info) in server_assets.assets.into_iter() {
        //     // prefer info.asset_id if set, otherwise use map key
        //     let asset_id = if !info.asset_id.is_empty() {
        //         info.asset_id.clone()
        //     } else {
        //         _key.clone()
        //     };
        //
        //     if !local_assets.contains_key(&asset_id) {
        //         // insert into local db
        //         let po = LocalPolyMarketAssetInfoPo {
        //             series_id: info.series_id.clone(),
        //             series_slug: info.series_slug.clone(),
        //             event_id: info.event_id.clone(),
        //             event_slug: info.event_slug.clone(),
        //             market_id: info.market_id.clone(),
        //             market_slug: info.market_slug.clone(),
        //             assert_id: asset_id.clone(),
        //             assert_slug: info.asset_slug.clone(),
        //         };
        //
        //         self.repository.create_asset(po).await?;
        //         res.insert(asset_id, 0u64);
        //     } else {
        //         let local_ts = *local_assets.get(&asset_id).unwrap_or(&0u64);
        //         if local_ts < info.latest_timestamp {
        //             res.insert(asset_id, local_ts);
        //         }
        //     }
        // }

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
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(server_msg) = rx.recv() => {
                        if let Some(payload) = server_msg.payload {
                            match payload {
                            Payload::PolymarketHistory(history) => {
                                    let batch_timestamp = history.timestamp;
                                    for h in history.history_list.into_iter() {
                                        let po = LocalPolyMarketHistoryPo::from_polymarket_history(h, batch_timestamp);
                                        do_batch_insert.insert_data(po).await;
                                    }

                                }
                            Payload::OkxKlineHistory(_) => {}}
                        }
                    }
                }
            }
        });

        Ok(tx)
    }
}

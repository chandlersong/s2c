use crate::config::get_config;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::sync_interface_client::SyncInterfaceClient;
use crate::sync::models::grpc_sync::{ServerMessage, SyncRequest};
use async_trait::async_trait;
use futures::Stream;
use governor::Jitter;
use log::{error, info};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};
use tonic::transport::{Channel, Endpoint};
use tonic::{Request, Status};

pub(crate) static GRPC_MANAGER: OnceLock<GrpcChannelManager> = OnceLock::new();

pub fn get_grpc_manager_from_config() -> GrpcChannelManager {
    GRPC_MANAGER
        .get_or_init(|| {
            let app_config = get_config();
            let sync_client_config = match &app_config.sync_client {
                None => {
                    panic!("No sync client config provide provided")
                }
                Some(config) => config,
            };
            // //FUTURE:改成https
            let server_url = format!("http://{}:{}", sync_client_config.server_host, sync_client_config.server_port);
            info!("连接到远程服务器:{}", server_url);
            // // 连接到 gRPC 服务（根据需要修改地址）-
            Arc::new(GrpcChannelManagerImpl::new(server_url.as_ref()))
        })
        .clone()
}

pub type GrpcServerMessageStream = Pin<Box<dyn Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

#[cfg_attr(any(test, feature = "mockable"), mockall::automock)]
#[async_trait]
pub trait GrpcChannelManagerTrait: Send + Sync {
    async fn get_channel(&self) -> Channel;
    async fn reconnect(&self) -> Channel;
    async fn connect(&self) -> Channel;
    async fn sync_history(&self, request: SyncRequest) -> Result<GrpcServerMessageStream, YuError>;
}

pub type GrpcChannelManager = Arc<dyn GrpcChannelManagerTrait + Send + Sync>;

struct ConnectionHolder {
    channel: OnceLock<Channel>,
    reconnect_notify: Notify,   // 通知等待者
    reconnect_mutex: Mutex<()>, // 防止多个进程同时重连
}

pub struct GrpcChannelManagerImpl {
    inner: Arc<ConnectionHolder>,
    server_url: String,
}

impl GrpcChannelManagerImpl {
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

#[async_trait]
impl GrpcChannelManagerTrait for GrpcChannelManagerImpl {
    async fn get_channel(&self) -> Channel {
        GrpcChannelManagerImpl::get_channel(self).await
    }

    async fn reconnect(&self) -> Channel {
        GrpcChannelManagerImpl::reconnect(self).await
    }

    async fn connect(&self) -> Channel {
        GrpcChannelManagerImpl::connect(self).await
    }

    async fn sync_history(&self, request: SyncRequest) -> Result<GrpcServerMessageStream, YuError> {
        let mut client = SyncInterfaceClient::new(self.connect().await);
        let stream = client.sync_history(Request::new(request)).await?.into_inner();
        Ok(Box::pin(stream))
    }
}

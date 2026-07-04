use crate::errors::YuError;
use crate::sync::sync_server::grpc_sync::{PolyMarketAssetTimestamp, ServerMessage};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::mpsc;

struct SyncClientService {
    pg_pool: &'static PgPool,
}

impl SyncClientService {
    pub fn new(pg_pool: &'static PgPool) -> Self {
        Self { pg_pool }
    }

    ///
    /// 校准本地数据库
    /// 1. 如果有新的asset info。那么就加入本地数据库
    /// 2. 返回一个map，为本地数据库的时间戳，和服务器端不一样的。
    ///
    pub async fn align_local_assets(&self, assert_info: PolyMarketAssetTimestamp) -> Result<HashMap<String, u64>, YuError> {
        todo!()
    }

    ///
    /// 监听数据，写入数据库
    ///
    pub async fn start_listen(rx: mpsc::Receiver<ServerMessage>) -> Result<(), YuError> {
        todo!()
    }
}

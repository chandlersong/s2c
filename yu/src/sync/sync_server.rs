use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::{DuckTableTableChannel, request_data_source_provider_from_table};
use crate::polymarket::database::get_polymarket_price_history_table;
use crate::polymarket::po::PolyMarketHistoryPo;
use crate::sync::sync_server::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::sync_server::grpc_sync::{
    AssetTimestamp, Empty, PolyMarketHistory, PolyMarketHistoryList, ServerMessage, SubscribeRequest, SyncRequest, server_message,
};
use log::{error, info};
use std::collections::HashMap;
use std::pin::Pin;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use yue::query_message::DataSourceProviderTrait;

pub mod grpc_sync {
    use std::collections::HashMap;

    tonic::include_proto!("grpc_sync");

    impl From<HashMap<String, u64>> for AssetTimestamp {
        fn from(value: HashMap<String, u64>) -> Self {
            let mut res = AssetTimestamp {
                timestamps: Default::default(),
            };
            for (k, v) in &value {
                res.timestamps.insert(k.clone(), v.clone());
            }
            res
        }
    }
}

enum SyncInternalCommand {
    QueryAssetTimestamp(oneshot::Sender<Result<AssetTimestamp, Status>>),
}

pub struct YuSyncServer {
    commands_sender: mpsc::Sender<SyncInternalCommand>,
}

///
/// 获取最新的asset id
///
pub async fn get_asset_timestamp(provider: DuckDBDSProvider) -> HashMap<String, u64> {
    // 最原始的做法：通过 provider 获取连接，直接用 stmt.query 返回 rows，然后在内存里计算每个 asset 的最大 timestamp
    let mut res: HashMap<String, u64> = HashMap::new();

    let sql_all = "SELECT assert_id, timestamp FROM poly_market_price_history ORDER BY assert_id, timestamp;";
    match provider.acquire() {
        Ok(conn) => {
            let mut stmt = match conn.prepare(sql_all) {
                Ok(s) => s,
                Err(e) => {
                    error!("prepare failed: {:?}", e);
                    return res;
                }
            };
            let mut rows = match stmt.query([]) {
                Ok(r) => r,
                Err(e) => {
                    error!("query failed: {:?}", e);
                    return res;
                }
            };

            while let Some(row_res) = rows.next().map_err(|e| e.to_string()).ok() {
                match row_res {
                    Some(row) => {
                        // 尝试按照常见类型获取 timestamp
                        let key: Result<String, _> = row.get(0);
                        if let Ok(k) = key {
                            // 尝试 i64 -> u64
                            let mut ts_opt: Option<u64> = None;
                            if let Ok(v) = row.get::<usize, i64>(1) {
                                ts_opt = Some(v as u64);
                            } else if let Ok(vu) = row.get::<usize, u64>(1) {
                                ts_opt = Some(vu);
                            } else if let Ok(sv) = row.get::<usize, String>(1) {
                                if let Ok(parsed) = sv.parse::<u64>() {
                                    ts_opt = Some(parsed);
                                }
                            }
                            if let Some(ts) = ts_opt {
                                let entry = res.entry(k).or_insert(0u64);
                                if *entry < ts {
                                    *entry = ts;
                                }
                            }
                        }
                    }
                    None => break,
                }
            }
        }
        Err(e) => error!("acquire provider failed: {:?}", e),
    }

    res
}

impl YuSyncServer {
    pub fn new(
        history_rx: broadcast::Receiver<PolyMarketHistory>,
        asset_timestamp: HashMap<String, u64>,
        table: Option<DuckTableTableChannel<PolyMarketHistoryPo>>,
    ) -> Self {
        let (commands_sender, commands_receiver) = mpsc::channel(10);
        let polymarket_table = table.unwrap_or(get_polymarket_price_history_table());
        tokio::spawn(async move { Self::run(commands_receiver, polymarket_table, asset_timestamp) });
        Self { commands_sender }
    }

    ///
    /// 关于这个服务，我觉得主要问题还是在于共享数据。
    /// 启动的时候，需要
    /// 1. 获取asset列表
    /// 2. 启动监听循环
    ///     1. 收到消息
    ///     2. 处理查询。
    ///
    async fn run(
        mut commands_rx: mpsc::Receiver<SyncInternalCommand>,
        polymarket_table: DuckTableTableChannel<PolyMarketHistoryPo>,
        asset_timestamp: HashMap<String, u64>,
    ) {
        let ds_provider = match request_data_source_provider_from_table(polymarket_table.clone()).await {
            Some(ds) => ds,
            None => return, //以后再说吧
        };
        info!("Sync server run loop started");

        loop {
            tokio::select! {
                command = commands_rx.recv() => {
                    match command {
                        Some(SyncInternalCommand::QueryAssetTimestamp(tx)) => {
                            let message: AssetTimestamp = AssetTimestamp::from(asset_timestamp.clone());
                            // ignore send error (receiver might be dropped)
                            if let Err(e) = tx.send(Ok(message)){
                                error!("send asset timestamp failed: {:?}", e);
                            }
                        }
                        None => {
                            // internal command channel closed,退出 loop
                            log::info!("Sync internal command channel closed, stopping run loop");
                            break;
                        }
                    }
                }
            }
        }
    }
}
#[tonic::async_trait]
impl SyncInterface for YuSyncServer {
    async fn get_latest_timestamps(&self, _request: Request<Empty>) -> Result<Response<AssetTimestamp>, Status> {
        let (tx, rx) = oneshot::channel();

        // 发送内部命令到后台 task
        if let Err(e) = self.commands_sender.send(SyncInternalCommand::QueryAssetTimestamp(tx)).await {
            error!("send query asset timestamp failed: {:?}", e);
            return Err(Status::internal("sync server not running"));
        }

        // 等待后台返回
        match rx.await {
            Ok(Ok(asset_ts)) => Ok(Response::new(asset_ts)),
            Ok(Err(status)) => Err(status),
            Err(e) => {
                error!("get asset timestamp failed,{}", e);
                Err(Status::internal("internal server error"))
            }
        }
    }

    type SyncHistoryStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn sync_history(&self, request: Request<SyncRequest>) -> Result<Response<Self::SyncHistoryStream>, Status> {
        request.get_ref().timestamp;
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let reply = ServerMessage {
            payload: Some(server_message::Payload::PolymarketHistory(PolyMarketHistoryList {
                history_list: Vec::new(),
                timestamp: 123,
            })),
        };
        if tx.send(Ok(reply)).await.is_err() {}

        Ok(Response::new(Box::pin(ReceiverStream::new(rx)) as Self::SyncHistoryStream))
    }

    type SubscribeLatestStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn subscribe_latest(&self, request: Request<SubscribeRequest>) -> Result<Response<Self::SubscribeLatestStream>, Status> {
        todo!()
    }
}

#[cfg(test)]
pub mod tests {
    use crate::duck_db::DuckDBDSProvider;
    use crate::duck_db_tables::{DuckDBOneTable, DuckTableTableChannel};
    use crate::polymarket::database::initial_tables;
    use crate::polymarket::db_consts::PolyMarketTables;
    use crate::polymarket::po::PolyMarketHistoryPo;
    use crate::test_utils::create_memory_db_provider;
    use yue::query_message::GetDataSourceProviderPayload;

    pub fn create_memory_table() -> (DuckDBDSProvider, DuckTableTableChannel<PolyMarketHistoryPo>) {
        let provider = create_memory_db_provider();
        let table = DuckDBOneTable::<PolyMarketHistoryPo, PolyMarketTables>::start_new(PolyMarketTables::PriceHistory, Some(provider.clone()));
        (provider, table)
    }

    ///
    /// 测试相应的获取数据库中，asset_timestamp.
    ///
    /// 测试步骤
    /// 1. 加入一些数据。
    ///     - assetA，两条数据，不同时间戳
    ///     - assetB，三条数据，不同时间戳
    ///
    /// 2. asset_timestamp，只是包含assetA和assetB，然后都是最新时间戳。
    ///
    ///
    #[tokio::test]
    pub async fn test_get_asset_timestamp() {
        use tokio::sync::oneshot;
        use yue::query_message::{BatchInsertPayload, QueryCommand};

        let (provider, table) = create_memory_table();

        // create table and wait for execution
        initial_tables(Some(provider.clone())).expect("initial tables failed");

        // prepare data
        let p1 = PolyMarketHistoryPo {
            assert_id: "ASSETA".to_string(),
            timestamp: 1000,
            payload: vec![],
        };
        let p2 = PolyMarketHistoryPo {
            assert_id: "ASSETA".to_string(),
            timestamp: 2000,
            payload: vec![],
        };
        let p3 = PolyMarketHistoryPo {
            assert_id: "ASSETB".to_string(),
            timestamp: 1500,
            payload: vec![],
        };
        let p4 = PolyMarketHistoryPo {
            assert_id: "ASSETB".to_string(),
            timestamp: 2500,
            payload: vec![],
        };
        let p5 = PolyMarketHistoryPo {
            assert_id: "ASSETB".to_string(),
            timestamp: 3000,
            payload: vec![],
        };

        // batch insert and wait for completion
        let po_vec = vec![p1, p2, p3, p4, p5];
        let (ins_tx, ins_rx) = oneshot::channel();
        if let Err(e) = table.send(QueryCommand::BatchInsert(BatchInsertPayload::new(po_vec, ins_tx))).await {
            panic!("batch insert send failed: {:?}", e);
        }
        match ins_rx.await {
            Ok(Ok(inserted)) => println!("batch insert returned inserted={}", inserted),
            Ok(Err(e)) => panic!("batch insert failed: {:?}", e),
            Err(e) => panic!("batch insert response error: {:?}", e),
        }

        // verify count == 5
        let (tx, rx) = oneshot::channel();
        if let Err(e) = table.send(QueryCommand::GetCount(tx)).await {
            panic!("get count send failed: {:?}", e);
        }
        match rx.await {
            Ok(Ok(count)) => println!("get_count returned {}", count),
            other => panic!("unexpected count result: {:?}", other),
        }

        // // debug: print all rows
        let (tx2, rx2) = oneshot::channel();

        if let Err(e) = table
            .send(QueryCommand::GetDataSourceProvider(GetDataSourceProviderPayload::new(tx2)))
            .await
        {
            panic!("send select all failed: {:?}", e);
        }
        let data_source = match rx2.await {
            Ok(Ok(ds)) => ds,
            other => panic!("select all unexpected: {:?}", other),
        };

        let asset_timestamp = super::get_asset_timestamp(data_source).await;
        assert_eq!(asset_timestamp.len(), 2);
        assert_eq!(asset_timestamp.get("ASSETA").copied(), Some(2000u64));
        assert_eq!(asset_timestamp.get("ASSETB").copied(), Some(3000u64));
    }
}

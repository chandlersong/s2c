use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::polymarket::database::get_polymarket_price_history_table;
use crate::polymarket::po::PolyMarketHistoryPo;
use crate::sync::sync_server::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::sync_server::grpc_sync::{
    AssetTimestamp, Empty, PolyMarketHistory, PolyMarketHistoryList, ServerMessage, SubscribeRequest, SyncRequest, server_message,
};
use log::{error, info};
use std::collections::HashMap;
use std::pin::Pin;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use yue::query_message::{DataSourceProviderTrait, InsertPayload, QueryCommand};

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

enum SyncInternalCommand {
    QueryAssetTimestamp(oneshot::Sender<Result<AssetTimestamp, Status>>),
}

pub struct YuSyncServer {
    commands_sender: mpsc::Sender<SyncInternalCommand>,
    history_tx: broadcast::Sender<PolyMarketHistory>,
}

impl YuSyncServer {
    pub async fn new(
        history_tx: broadcast::Sender<PolyMarketHistory>,
        asset_timestamp: HashMap<String, u64>,
        table: Option<DuckTableTableChannel<PolyMarketHistoryPo>>,
    ) -> Self {
        let (commands_sender, commands_receiver) = mpsc::channel(10);
        let polymarket_table = table.unwrap_or(get_polymarket_price_history_table());
        let history_rx = history_tx.subscribe();
        tokio::spawn(async move { Self::run(commands_receiver, polymarket_table, asset_timestamp, history_rx).await });
        Self { commands_sender, history_tx }
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
        mut asset_timestamp: HashMap<String, u64>,
        mut history_rx: broadcast::Receiver<PolyMarketHistory>,
    ) {
        // let ds_provider = loop {
        //     match request_data_source_provider_from_table(polymarket_table.clone()).await {
        //         Some(ds) => break ds,
        //         None => {
        //             error!("request_data_source_provider_from_table returned None, retrying in 1s");
        //             tokio::time::sleep(Duration::from_secs(1)).await;
        //         }
        //     }
        // };
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
                            info!("Sync internal command channel closed, stopping run loop");
                            break;
                        }
                    }
                }
                history = history_rx.recv() => {
                    match history {
                        Ok(poly_market_history) => {
                            let latest_timestamp = poly_market_history.timestamp;
                            let assert_id = poly_market_history.asset_id.clone();
                            asset_timestamp.insert(assert_id,latest_timestamp);
                            let po = PolyMarketHistoryPo::from(poly_market_history);
                            let command = QueryCommand::Insert(InsertPayload::new_no_replay(po));
                            if let Err(e) = polymarket_table.send(command).await {
                                error!("send insert to polymarket_table failed: {:?}", e);
                            }
                        }
                        Err(e) => {
                            error!("recv history failed: {:?}", e);
                        }
                    }

                }
            }
        }
    }

    /// Send batched PolyMarketHistory messages to a client.
    ///
    /// 说明（中文）:
    /// - 将从 `history_receiver` 接收到的 `PolyMarketHistory` 按批次缓存，满足下列任一条件时把批次打包并发送给 `client_sender`：
    ///   1. 缓存条目数达到 `max_cache_size`；
    ///   2. 自上次发送后经过了 `max_loop_mill_seconds` 毫秒。
    /// - 错误处理策略：对 `broadcast::RecvError::Lagged` 忽略滞后消息，对 `Closed` 置位关闭标志并退出循环；若 `client_sender` 关闭（客户端断开），则停止发送并退出。
    ///
    /// 参数：
    /// - `history_receiver`: 广播订阅者，接收来自服务的 PolyMarketHistory
    /// - `client_sender`: 将封装好的 `ServerMessage` 发送回客户端的 mpsc 发送端
    /// - `max_cache_size`: 达到此数量时立即触发发送
    /// - `max_loop_mill_seconds`: 达到此时间（毫秒）时触发发送
    async fn send_history_to_client(
        mut history_receiver: broadcast::Receiver<PolyMarketHistory>,
        client_sender: mpsc::Sender<Result<ServerMessage, Status>>,
        max_cache_size: usize,
        max_loop_mill_seconds: usize,
    ) {
        let max_dur = Duration::from_millis(max_loop_mill_seconds as u64);

        loop {
            let mut buffer: Vec<PolyMarketHistory> = Vec::new();
            let deadline = Instant::now() + max_dur;
            let mut closed = false;

            // collect until size reached or timeout
            while buffer.len() < max_cache_size {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                let remaining = deadline - now;
                match tokio::time::timeout(remaining, history_receiver.recv()).await {
                    Ok(Ok(item)) => {
                        buffer.push(item);
                        continue;
                    }
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                        // skip lagged messages
                        continue;
                    }
                    Ok(Err(broadcast::error::RecvError::Closed)) => {
                        closed = true;
                        break;
                    }
                    Err(_) => {
                        // timeout waiting for next message
                        break;
                    }
                }
            }

            if buffer.is_empty() {
                if closed {
                    break;
                }
                // nothing collected, continue loop to wait again
                continue;
            }

            let list = PolyMarketHistoryList {
                history_list: buffer,
                timestamp: 0,
            };
            let message = ServerMessage {
                payload: Some(server_message::Payload::PolymarketHistory(list)),
            };

            if client_sender.send(Ok(message)).await.is_err() {
                // client disconnected
                break;
            }

            if closed {
                break;
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

    async fn subscribe_latest(&self, _request: Request<SubscribeRequest>) -> Result<Response<Self::SubscribeLatestStream>, Status> {
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(1000);
        let history_rx = self.history_tx.subscribe();
        tokio::spawn(async move {
            Self::send_history_to_client(history_rx, tx, 100, 1000).await;
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx)) as Self::SyncHistoryStream))
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

    /// Unit test: test_send_history_to_client
    ///
    /// 目的：验证基本批处理行为。
    /// 步骤：
    /// 1. 启动 `send_history_to_client`，设置 `max_cache_size=3` 和较短的超时；
    /// 2. 向 broadcast channel 发送 3 条 `PolyMarketHistory`；
    /// 3. 断言客户端收到一个包含 3 条记录的 `PolymarketHistory` 批次消息。
    #[tokio::test]
    pub async fn test_send_history_to_client() {
        use super::grpc_sync::{PolyMarketHistory, server_message};
        use tokio::sync::{broadcast, mpsc};
        use tonic::Status;

        let (tx, _) = broadcast::channel(16);
        let rx = tx.subscribe();
        let (client_tx, mut client_rx) = mpsc::channel::<Result<super::grpc_sync::ServerMessage, Status>>(10);

        // run sender task
        tokio::spawn(async move {
            super::YuSyncServer::send_history_to_client(rx, client_tx, 3, 500).await;
        });

        // send three messages
        let m1 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "A".to_string(),
            asset_slug: "".to_string(),
            timestamp: 1,
            price: 1.0,
        };
        let m2 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "B".to_string(),
            asset_slug: "".to_string(),
            timestamp: 2,
            price: 2.0,
        };
        let m3 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "C".to_string(),
            asset_slug: "".to_string(),
            timestamp: 3,
            price: 3.0,
        };

        tx.send(m1).unwrap();
        tx.send(m2).unwrap();
        tx.send(m3).unwrap();

        // expect aggregated message
        let received = client_rx.recv().await.expect("expected server message");
        match received {
            Ok(msg) => {
                if let Some(server_message::Payload::PolymarketHistory(list)) = msg.payload {
                    assert_eq!(list.history_list.len(), 3);
                } else {
                    panic!("unexpected payload");
                }
            }
            Err(e) => panic!("send error: {:?}", e),
        }
    }

    /// Unit test: test_send_history_triggers_on_count
    ///
    /// 目的：验证当接收到的消息数量达到 `max_cache_size` 时会立即触发发送（不依赖超时）。
    /// 步骤：
    /// 1. 启动 `send_history_to_client`，设置 `max_cache_size=2` 且将超时设置为较长；
    /// 2. 连续发送 2 条消息；
    /// 3. 断言客户端在短时间内收到一个包含 2 条记录的批次。
    #[tokio::test]
    pub async fn test_send_history_triggers_on_count() {
        use super::grpc_sync::PolyMarketHistory;
        use tokio::sync::{broadcast, mpsc};
        use tonic::Status;

        let (tx, _) = broadcast::channel(16);
        let rx = tx.subscribe();
        let (client_tx, mut client_rx) = mpsc::channel::<Result<super::grpc_sync::ServerMessage, Status>>(10);

        tokio::spawn(async move {
            super::YuSyncServer::send_history_to_client(rx, client_tx, 2, 5000).await;
        });

        let m1 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "A".to_string(),
            asset_slug: "".to_string(),
            timestamp: 1,
            price: 1.0,
        };
        let m2 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "B".to_string(),
            asset_slug: "".to_string(),
            timestamp: 2,
            price: 2.0,
        };

        tx.send(m1).unwrap();
        tx.send(m2).unwrap();

        let received = tokio::time::timeout(std::time::Duration::from_millis(500), client_rx.recv())
            .await
            .expect("timeout waiting")
            .expect("expected message");
        match received {
            Ok(msg) => {
                if let Some(super::grpc_sync::server_message::Payload::PolymarketHistory(list)) = msg.payload {
                    assert_eq!(list.history_list.len(), 2);
                } else {
                    panic!("unexpected payload");
                }
            }
            Err(e) => panic!("send error: {:?}", e),
        }
    }

    /// Unit test: test_send_history_triggers_on_timeout
    ///
    /// 目的：验证未达到数量阈值但超过时间阈值时会触发发送。
    /// 步骤：
    /// 1. 启动 `send_history_to_client`，设置 `max_cache_size` 为较大值、`max_loop_mill_seconds` 为较小值；
    /// 2. 发送少于 `max_cache_size` 的消息（例如 2 条）；
    /// 3. 等待超时并断言客户端收到包含这些消息的批次。
    #[tokio::test]
    pub async fn test_send_history_triggers_on_timeout() {
        use super::grpc_sync::PolyMarketHistory;
        use tokio::sync::{broadcast, mpsc};
        use tonic::Status;

        let (tx, _) = broadcast::channel(16);
        let rx = tx.subscribe();
        let (client_tx, mut client_rx) = mpsc::channel::<Result<super::grpc_sync::ServerMessage, Status>>(10);

        // timeout set small
        tokio::spawn(async move {
            super::YuSyncServer::send_history_to_client(rx, client_tx, 10, 100).await;
        });

        let m1 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "A".to_string(),
            asset_slug: "".to_string(),
            timestamp: 1,
            price: 1.0,
        };
        let m2 = PolyMarketHistory {
            series_id: "".to_string(),
            series_slug: "".to_string(),
            event_id: "".to_string(),
            event_slug: "".to_string(),
            market_id: "".to_string(),
            market_slug: "".to_string(),
            asset_id: "B".to_string(),
            asset_slug: "".to_string(),
            timestamp: 2,
            price: 2.0,
        };

        tx.send(m1).unwrap();
        tx.send(m2).unwrap();

        // wait for message triggered by timeout
        let received = tokio::time::timeout(std::time::Duration::from_secs(1), client_rx.recv())
            .await
            .expect("timeout waiting")
            .expect("expected message");
        match received {
            Ok(msg) => {
                if let Some(super::grpc_sync::server_message::Payload::PolymarketHistory(list)) = msg.payload {
                    assert_eq!(list.history_list.len(), 2);
                } else {
                    panic!("unexpected payload");
                }
            }
            Err(e) => panic!("send error: {:?}", e),
        }
    }
}

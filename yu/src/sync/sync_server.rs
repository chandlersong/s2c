use crate::duck_db::DuckDBDSProvider;
use crate::sync::sync_server::grpc_sync::client_message::Payload as ClientPayload;
use crate::sync::sync_server::grpc_sync::server_message::Payload as ServerPayload;
use crate::sync::sync_server::grpc_sync::sync_server_server::SyncServer;
use crate::sync::sync_server::grpc_sync::{ClientMessage, PolyMarketHistoryList, ServerMessage};
use log::error;
use std::collections::HashMap;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::{Request, Response, Status, Streaming};
use yue::query_message::DataSourceProviderTrait;

pub mod grpc_sync {
    tonic::include_proto!("grpc_sync");
}

#[derive(Default)]
pub struct YuSyncServer {}

///
/// 获取最新的asset id
///
async fn get_asset_timestamp(provider: DuckDBDSProvider) -> HashMap<String, u64> {
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
    pub fn new() -> Self {
        Self {}
    }

    ///
    /// 关于这个服务，我觉得主要问题还是在于共享数据。
    /// 启动的时候，需要
    /// 1. 获取asset列表
    /// 2. 启动监听循环
    ///     1. 收到消息
    ///     2. 处理查询。
    ///
    pub async fn start_polymarket_server() {
        todo!()
    }
}
#[tonic::async_trait]
impl SyncServer for YuSyncServer {
    type syncStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn sync(&self, request: Request<Streaming<ClientMessage>>) -> Result<Response<Self::syncStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);

        tokio::spawn(async move {
            while let Some(next_msg) = inbound.next().await {
                match next_msg {
                    Ok(msg) => {
                        if let Some(ClientPayload::Initial(_)) = msg.payload {
                            let reply = ServerMessage {
                                payload: Some(ServerPayload::PolymarketHistory(PolyMarketHistoryList {
                                    history_list: Vec::new(),
                                    timestamp: 123,
                                })),
                            };
                            if tx.send(Ok(reply)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(status) => {
                        let _ = tx.send(Err(status)).await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx)) as Self::syncStream))
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

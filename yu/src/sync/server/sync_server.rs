use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::{DuckDbTableTrait, DuckTableTableChannel, request_data_source_provider_from_table};
use crate::polymarket::database::get_polymarket_price_history_table;
use crate::polymarket::db_consts::PolyMarketTables::PriceHistory;
use crate::polymarket::po::{PolyMarketHistoryPo, PolyMarketInstrumentPo};
use crate::sync::models::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::models::grpc_sync::{
    Empty, InstrumentInfoList, PolyMarketHistory, PolyMarketHistoryList, PolymarketInstrument, ServerMessage, SubscribeRequest, SyncRequest,
    server_message,
};
use duckdb::params;
use li::tools::time::unix_time_now_u64_utc_seconds;
use log::{error, info};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tonic::{Request, Response, Status};
use yue::okx::models::common::InstrumentInfo;
use yue::query_message::{DataSourceProviderTrait, InsertPayload, QueryCommand};
// for decoding prost-encoded payloads into PolyMarketHistory

///
/// 获取最新的asset id
///
pub async fn get_asset_timestamp(provider: DuckDBDSProvider) -> HashMap<String, u64> {
    // 最原始的做法：通过 provider 获取连接，直接用 stmt.query 返回 rows，然后在内存里计算每个 asset 的最大 timestamp
    let mut res: HashMap<String, u64> = HashMap::new();

    let sql_all = "SELECT asset_id, timestamp FROM polymarket_price_history ORDER BY asset_id, timestamp;";
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
    QueryAssetTimestamp(oneshot::Sender<Result<InstrumentInfo, Status>>),
}

pub struct YuSyncServer {
    commands_sender: mpsc::Sender<SyncInternalCommand>,
    history_tx: broadcast::Sender<PolyMarketHistory>,
    ds_provider: DuckDBDSProvider,
    batch_size: usize,
}

impl YuSyncServer {
    pub async fn new(
        history_tx: broadcast::Sender<PolyMarketHistory>,
        asset_timestamp: HashMap<String, u64>,
        table: Option<DuckTableTableChannel<PolyMarketHistoryPo>>,
        asset_infos: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
        batch_size: usize,
    ) -> Self {
        let (commands_sender, commands_receiver) = mpsc::channel(10);
        let polymarket_table = table.unwrap_or(get_polymarket_price_history_table());
        let history_rx = history_tx.subscribe();
        let ds_provider = Self::get_ds_from_table(&polymarket_table).await;
        tokio::spawn(async move { Self::run(commands_receiver, polymarket_table, asset_timestamp, history_rx, asset_infos).await });
        Self {
            commands_sender,
            history_tx,
            ds_provider,
            batch_size,
        }
    }

    async fn get_ds_from_table(table: &DuckTableTableChannel<PolyMarketHistoryPo>) -> DuckDBDSProvider {
        // FUTURE: 加入一个试错的上限
        loop {
            match request_data_source_provider_from_table(table.clone()).await {
                Some(ds) => break ds,
                None => {
                    error!("request_data_source_provider_from_table returned None, retrying in 1s");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
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
        asset_infos: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    ) {
        info!("Sync server run loop started");
        todo!("1. 获取asset列表，2. 启动监听循环，3. 收到消息，4. 处理查询。");
    }

    ///
    /// 1. 从数据库里面，从表poly_market_price_history里面大于start_timestamp里面所有的数据
    ///    - 一次最多查询max_batch_size，要把所有的大于start_timestamp的查询出来
    /// 2. 然后按照max_batch_size最大的一组，发送给客户端
    ///
    pub async fn query_and_send_history(
        provider: DuckDBDSProvider,
        asset_id: &str,
        start_timestamp: u64,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
        max_batch_size: usize,
    ) {
        // 使用内存/文件数据库提供者从 poly_market_price_history 中查询指定 asset_id
        // 注意：为了兼容 duckdb 参数绑定的不确定性，这里将 asset_id 做简单的 SQL 转义后拼接入查询语句
        // 查询逻辑：按 timestamp 升序，查询 > start_timestamp 的记录，限制为 max_batch_size

        // 如果 max_batch_size 为 0，避免除零或无限循环，直接返回
        if max_batch_size == 0 {
            let list = PolyMarketHistoryList {
                history_list: vec![],
                timestamp: 0,
            };
            let message = ServerMessage {
                payload: Some(server_message::Payload::PolymarketHistory(list)),
            };
            let _ = tx.send(Ok(message)).await;
            return;
        }

        let mut offset: i64 = 0;
        let batch = max_batch_size as usize;
        let mut any_sent = false;

        loop {
            let sql = format!(
                "SELECT * FROM {} WHERE asset_id = '{}' AND timestamp > {} ORDER BY timestamp LIMIT {} OFFSET ?",
                PriceHistory.table_name(),
                asset_id.replace("'", "''"),
                start_timestamp,
                batch
            );

            match provider.acquire() {
                Ok(conn) => {
                    let mut stmt = match conn.prepare(sql.as_str()) {
                        Ok(s) => s,
                        Err(e) => {
                            error!("prepare query_and_send_history failed: {:?}", e);
                            return;
                        }
                    };

                    // 执行带分页的查询，绑定 offset 参数
                    let mapped_iter = match stmt.query_map(params![offset as i64], |row| {
                        Ok(PolyMarketHistory {
                            inst_id: row.get("asset_id")?,
                            timestamp: row.get("timestamp")?,
                            price: row.get("price")?,
                        })
                    }) {
                        Ok(it) => it,
                        Err(e) => {
                            error!("query_map failed in query_and_send_history: {:?}", e);
                            let _ = tx.send(Err(Status::internal("db query failed"))).await;
                            return;
                        }
                    };

                    // 收集本页结果并处理逐行映射错误
                    let mut results: Vec<PolyMarketHistory> = Vec::new();
                    for row_res in mapped_iter {
                        match row_res {
                            Ok(pm) => results.push(pm),
                            Err(e) => {
                                error!("row mapping failed in query_and_send_history: {:?}", e);
                                let _ = tx.send(Err(Status::internal("db row mapping failed"))).await;
                                return;
                            }
                        }
                    }

                    let row_count = results.len();

                    drop(stmt);
                    drop(conn);

                    // 如果本次查询返回了数据，就发送批次
                    if !results.is_empty() {
                        any_sent = true;
                        let list = PolyMarketHistoryList {
                            history_list: results,
                            timestamp: unix_time_now_u64_utc_seconds(),
                        };
                        let message = ServerMessage {
                            payload: Some(server_message::Payload::PolymarketHistory(list)),
                        };
                        if tx.send(Ok(message)).await.is_err() {
                            error!("client receiver closed when sending history");
                            return;
                        }
                    }

                    // 若本轮返回行数少于 batch，说明已到末尾，退出循环
                    if row_count < batch {
                        break;
                    }

                    // 否则继续下一页
                    offset += batch as i64;
                }
                Err(e) => {
                    error!("acquire provider failed in query_and_send_history: {:?}", e);
                    let _ = tx.send(Err(Status::internal("failed to acquire datasource"))).await;
                    return;
                }
            }
        }

        // 如果没有任何记录被发送，则发送一个空批次，方便客户端判断
        if !any_sent {
            let list = PolyMarketHistoryList {
                history_list: vec![],
                timestamp: unix_time_now_u64_utc_seconds(),
            };
            let message = ServerMessage {
                payload: Some(server_message::Payload::PolymarketHistory(list)),
            };
            let _ = tx.send(Ok(message)).await;
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
                timestamp: unix_time_now_u64_utc_seconds(),
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
    async fn get_instrument_info(&self, request: Request<Empty>) -> Result<Response<InstrumentInfoList>, Status> {
        todo!()
    }

    type SyncHistoryStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn sync_history(&self, request: Request<SyncRequest>) -> Result<Response<Self::SyncHistoryStream>, Status> {
        todo!()
    }

    type SubscribeLatestStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn subscribe_latest(&self, _request: Request<SubscribeRequest>) -> Result<Response<Self::SubscribeLatestStream>, Status> {
        todo!()
    }
}

#[cfg(test)]
pub mod tests {}

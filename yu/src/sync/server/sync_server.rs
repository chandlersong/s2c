use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::okx::service::OptionService;
use crate::polymarket::po::PolyMarketHistoryPo;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::models::grpc_sync::{
    Empty, InstrumentList, PolyMarketHistory, PolyMarketHistoryList, PolymarketInstrument, ServerMessage, SubscribeRequest, SyncRequest,
    server_message,
};
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info, trace};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tonic::{Request, Response, Status};
use yue::query_message::DataSourceProviderTrait;
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
    Subscribe(SubscribePayload),
}

struct SubscribePayload {
    id: u64,
    tx: mpsc::Sender<Result<ServerMessage, Status>>,
}

pub struct YuSyncServer {
    polymarket_history_service: SeriesHistoryMarketService,
    okx_option_service: Arc<OptionService>,
    batch_size: usize,
    command_channel: mpsc::Sender<SyncInternalCommand>,
}

impl YuSyncServer {
    pub async fn create_and_start(
        polymarket_history_service: SeriesHistoryMarketService,
        okx_option_service: Arc<OptionService>,
    ) -> Result<Self, YuError> {
        let (command_channel, command_rx) = mpsc::channel::<SyncInternalCommand>(100);

        let polymarket_history_receiver = polymarket_history_service.subscribe_history_broadcast();
        Self::start_broadcast_history(command_rx, polymarket_history_receiver, 500, 1000).await?;
        Ok(Self {
            polymarket_history_service,
            okx_option_service,
            batch_size: 500,
            command_channel,
        })
    }

    /// Send batched PolyMarketHistory messages to a clients.
    ///
    /// 主要的作用是把接收到的历史消息，统一发送给客户端。
    /// 因为考虑到多客户端的情况。所以在一个线程里面，统一的分发。而不是每个客户端都单独去接收广播消息。
    ///
    ///
    /// 说明（中文）:
    /// - 将从 `polymarket_history_receiver` 接收到的 `PolyMarketHistoryPo`，转换成PolyMarketHistory，按批次缓存，满足下列任一条件时把批次打包并发送给 `client_sender`：
    ///   1. 缓存条目数达到 `max_cache_size`；
    ///   2. 自上次发送后经过了 `max_loop_mill_seconds` 毫秒。
    /// - 发送时，检查对应的channel是否关闭，如果关闭，则删除出缓存的history_tx_map，并打印认知
    /// - 收到Subscribe的信息，把发布的tx，放入history_tx_map，key为id，value为tx。进行缓存
    ///
    /// 参数：
    /// - `command_rx`: 接收订阅命令的 mpsc 接收端
    /// - `polymarket_history_receiver`: 接收来自服务的 PolyMarketHistory 的 mpsc 接收端
    /// - `max_cache_size`: 达到此数量时立即触发发送
    /// - `max_loop_mill_seconds`: 达到此时间（毫秒）时触发发送

    async fn start_broadcast_history(
        mut command_rx: mpsc::Receiver<SyncInternalCommand>,
        mut polymarket_history_receiver: broadcast::Receiver<PolyMarketHistoryPo>,
        max_cache_size: usize,
        max_loop_mill_seconds: usize,
    ) -> Result<(), YuError> {
        tokio::spawn(async move {
            info!("开启广播历史数据的任务");
            // 临时缓存单条 history（proto）用于批量发送
            let mut batch_buffer: Vec<PolyMarketHistory> = Vec::with_capacity(max_cache_size);
            let mut last_broadcast_timestamp: u64 = unix_time_now_u64_utc();
            let mut history_tx_map: HashMap<u64, mpsc::Sender<Result<ServerMessage, Status>>> = HashMap::new();

            loop {
                tokio::select! {
                    biased;
                    recv_res = polymarket_history_receiver.recv() => {
                        match recv_res {
                            Ok(po) => {
                                trace!("Received history broadcast with timestamp: {}", po.timestamp);
                                // 转成 proto 并缓存
                                let item = PolyMarketHistory {
                                    inst_id: po.instrument_id,
                                    timestamp: po.timestamp,
                                    price: po.price,
                                };
                                batch_buffer.push(item);
                            }
                            Err(e) => {
                                error!("history broadcast receiver error: {:?}", e);
                                // 如果没有活跃发送者，短暂休眠再试
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    Some(command) = command_rx.recv() => {
                        match command {
                            SyncInternalCommand::Subscribe(payload) => {
                                info!("Received subscribe command with id: {}", payload.id);
                                history_tx_map.insert(payload.id, payload.tx);
                            }
                        }
                    }
                    () = tokio::time::sleep(tokio::time::Duration::from_millis(200)) => {
                        // 心跳，继续到后面的 flush 检查
                        trace!("no history in last 200 milliseconds");
                    }
                }

                // 检查是否需要触发发送：缓存大小或时间间隔
                let now_ms = unix_time_now_u64_utc();
                let elapsed_ms = now_ms.saturating_sub(last_broadcast_timestamp);
                if !batch_buffer.is_empty() && (batch_buffer.len() >= max_cache_size || elapsed_ms as usize >= max_loop_mill_seconds) {
                    // 打包成一条 ServerMessage
                    let list = PolyMarketHistoryList {
                        history_list: batch_buffer.clone(),
                        timestamp: unix_time_now_u64_utc(),
                    };
                    let msg = ServerMessage {
                        payload: Some(server_message::Payload::PolymarketHistory(list)),
                    };

                    // 发送给所有订阅者，检测并清理已关闭的channel
                    let mut remove_keys: Vec<u64> = Vec::new();
                    for (id, tx) in history_tx_map.iter_mut() {
                        if tx.is_closed() {
                            info!("Subscriber {} channel is closed, removing from list", id);
                            remove_keys.push(*id);
                            continue;
                        }
                        // 克隆消息并发送
                        let send_res = tx.send(Ok(msg.clone())).await;
                        if let Err(e) = send_res {
                            error!("Failed to send history to subscriber {}: {:?}", id, e);
                            // 发送失败通常表示接收方已关闭
                            remove_keys.push(*id);
                        }
                    }

                    for k in remove_keys {
                        history_tx_map.remove(&k);
                    }

                    // 更新状态并清空缓存
                    last_broadcast_timestamp = now_ms;
                    batch_buffer.clear();
                }
            }
        });
        Ok(())
    }
}
#[tonic::async_trait]
impl SyncInterface for YuSyncServer {
    async fn list_instrument(&self, _request: Request<Empty>) -> Result<Response<InstrumentList>, Status> {
        let polymarket_instruments = self.polymarket_history_service.list_instruments().await;
        let okx_option_instruments = self.okx_option_service.list_instruments().await;
        let mut instruments_map: HashMap<String, crate::sync::models::grpc_sync::Instrument> = HashMap::new();

        match polymarket_instruments {
            Ok(list) => {
                for inst in list {
                    // 直接把 PolyMarketInstrumentPo 转换为 proto，latest_timestamp 暂时置为 0
                    let asset_id = inst.asset_id.clone();
                    let poly = PolymarketInstrument::from(inst);

                    let instrument = crate::sync::models::grpc_sync::Instrument {
                        payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Polymarket(poly)),
                    };
                    instruments_map.insert(asset_id, instrument);
                }
            }
            Err(e) => {
                error!("list_instruments error: {:?}", e);
                return Err(Status::internal(format!("list polymarket instruments error: {:?}", e)));
            }
        };

        match okx_option_instruments {
            Ok(okx_list) => {
                for inst in okx_list {
                    let key = inst.inst_identify.clone();
                    let okx = crate::sync::models::grpc_sync::OkxInstrument::from(inst);
                    let instrument = crate::sync::models::grpc_sync::Instrument {
                        payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Okx(okx)),
                    };
                    instruments_map.insert(key, instrument);
                }
            }
            Err(e) => {
                error!("list_okx_instruments error: {:?}", e);
                return Err(Status::internal(format!("list okx option instruments error: {:?}", e)));
            }
        };

        Ok(Response::new(InstrumentList {
            instruments: instruments_map,
        }))
    }

    type SyncHistoryStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn sync_history(&self, request: Request<SyncRequest>) -> Result<Response<Self::SyncHistoryStream>, Status> {
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let query_service = self.polymarket_history_service.clone();
        let inst_id = request.get_ref().inst_id.clone();
        let timestamp = request.get_ref().timestamp.clone();
        let batch_size = self.batch_size;

        // Spawn a task to query history and stream results back through tx
        tokio::spawn(async move {
            match query_service.query_instrument_history(inst_id, timestamp).await {
                Ok(history_vec) => {
                    if history_vec.is_empty() {
                        // send an empty list once
                        let list = PolyMarketHistoryList {
                            history_list: vec![],
                            timestamp: unix_time_now_u64_utc(),
                        };
                        let msg = ServerMessage {
                            payload: Some(server_message::Payload::PolymarketHistory(list)),
                        };
                        let _ = tx.send(Ok(msg)).await;
                        return;
                    }

                    for chunk in history_vec.chunks(batch_size) {
                        let mut histories: Vec<PolyMarketHistory> = Vec::with_capacity(chunk.len());
                        for h in chunk.iter() {
                            histories.push(PolyMarketHistory {
                                inst_id: h.instrument_id,
                                timestamp: h.timestamp,
                                price: h.price,
                            });
                        }
                        let list = PolyMarketHistoryList {
                            history_list: histories,
                            timestamp: unix_time_now_u64_utc(),
                        };
                        let msg = ServerMessage {
                            payload: Some(server_message::Payload::PolymarketHistory(list)),
                        };
                        if tx.send(Ok(msg)).await.is_err() {
                            // receiver dropped, stop streaming
                            break;
                        }
                    }
                }
                Err(e) => {
                    let status = Status::internal(format!("query history error: {:?}", e));
                    let _ = tx.send(Err(status)).await;
                }
            }
        });

        Ok(Response::new(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)) as Self::SyncHistoryStream
        ))
    }

    type SubscribeLatestStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    async fn subscribe_latest(&self, _request: Request<SubscribeRequest>) -> Result<Response<Self::SubscribeLatestStream>, Status> {
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let request = _request.into_inner();
        //FUTURE: 以后做点session管理之类工作
        let payload = SubscribePayload {
            id: request.client_id, // You can set this to a unique ID if needed
            tx,
        };

        if let Err(e) = self.command_channel.send(SyncInternalCommand::Subscribe(payload)).await {
            error!("Failed to send subscribe command: {:?}", e);
            return Err(Status::internal("Failed to subscribe"));
        }
        Ok(Response::new(
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)) as Self::SyncHistoryStream
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{Duration, timeout};

    // 1) 缓存达到大小时立即发送
    #[tokio::test]
    async fn test_batch_send_on_size() {
        let (command_tx, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        let (poly_tx, poly_rx) = broadcast::channel::<PolyMarketHistoryPo>(16);

        // spawn the broadcaster
        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, 3, 10_000).await.unwrap();

        // create a client and subscribe
        let (client_tx, mut client_rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let payload = SubscribePayload { id: 1u64, tx: client_tx };
        command_tx.send(SyncInternalCommand::Subscribe(payload)).await.unwrap();
        // give background task a moment to register the subscriber
        tokio::time::sleep(Duration::from_millis(50)).await;

        // send 3 history messages to trigger size-based flush
        for i in 0..3 {
            let po = PolyMarketHistoryPo {
                instrument_id: i + 1,
                timestamp: unix_time_now_u64_utc(),
                price: 1.0 + i as f64,
            };
            poly_tx.send(po).unwrap();
        }

        // expect one batched message
        let opt = timeout(Duration::from_secs(3), client_rx.recv())
            .await
            .expect("timeout waiting for message");
        let msg = opt.expect("stream closed").expect("status error");
        match msg.payload {
            Some(server_message::Payload::PolymarketHistory(list)) => {
                assert_eq!(list.history_list.len(), 3, "expected 3 history items in batch");
            }
            _ => panic!("unexpected payload"),
        }
    }

    // 2) 时间触发发送（未达到大小但超过时间间隔）
    #[tokio::test]
    async fn test_time_based_flush() {
        let (command_tx, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        let (poly_tx, poly_rx) = broadcast::channel::<PolyMarketHistoryPo>(16);

        // use small time threshold (ms)
        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, 10, 800).await.unwrap();

        let (client_tx, mut client_rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let payload = SubscribePayload { id: 2u64, tx: client_tx };
        command_tx.send(SyncInternalCommand::Subscribe(payload)).await.unwrap();

        // send 2 messages, below size threshold
        for i in 0..2 {
            let po = PolyMarketHistoryPo {
                instrument_id: 100 + i,
                timestamp: unix_time_now_u64_utc(),
                price: 2.0 + i as f64,
            };
            poly_tx.send(po).unwrap();
        }

        // wait for time-based flush (allow generous timeout)
        let opt = timeout(Duration::from_secs(5), client_rx.recv())
            .await
            .expect("timeout waiting for time flush");
        let msg = opt.expect("stream closed").expect("status error");
        match msg.payload {
            Some(server_message::Payload::PolymarketHistory(list)) => {
                assert_eq!(list.history_list.len(), 2, "expected 2 history items in time-based batch");
            }
            _ => panic!("unexpected payload"),
        }
    }

    // 3) 已关闭的订阅者应被清理，不影响发送
    #[tokio::test]
    async fn test_remove_closed_subscriber() {
        let (command_tx, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        let (poly_tx, poly_rx) = broadcast::channel::<PolyMarketHistoryPo>(16);

        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, 1, 10_000).await.unwrap();

        // active subscriber
        let (active_tx, mut active_rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let payload_active = SubscribePayload { id: 10u64, tx: active_tx };
        command_tx.send(SyncInternalCommand::Subscribe(payload_active)).await.unwrap();

        // closed subscriber: create rx and drop it so tx.is_closed() == true
        let (closed_tx, closed_rx) = mpsc::channel::<Result<ServerMessage, Status>>(1);
        drop(closed_rx); // close receiver side
        let payload_closed = SubscribePayload { id: 11u64, tx: closed_tx };
        command_tx.send(SyncInternalCommand::Subscribe(payload_closed)).await.unwrap();

        // give background task a moment to register subscribers
        tokio::time::sleep(Duration::from_millis(50)).await;

        // send one item to trigger immediate flush (max_cache_size = 1)
        let po = PolyMarketHistoryPo {
            instrument_id: 999,
            timestamp: unix_time_now_u64_utc(),
            price: 9.99,
        };
        poly_tx.send(po).unwrap();

        // active should receive it
        let opt = timeout(Duration::from_secs(3), active_rx.recv())
            .await
            .expect("timeout waiting for active subscriber");
        let msg = opt.expect("stream closed").expect("status error");
        match msg.payload {
            Some(server_message::Payload::PolymarketHistory(list)) => assert_eq!(list.history_list.len(), 1),
            _ => panic!("unexpected payload"),
        }

        // If we reached here without panic, closed subscriber was cleaned up and did not block
    }

    // 4) 订阅在任务启动之后注册也能收到后续广播
    #[tokio::test]
    async fn test_subscribe_after_start() {
        let (command_tx, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        let (poly_tx, poly_rx) = broadcast::channel::<PolyMarketHistoryPo>(16);

        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, 1, 10_000).await.unwrap();

        // register subscriber after start
        let (client_tx, mut client_rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let payload = SubscribePayload { id: 42u64, tx: client_tx };
        command_tx.send(SyncInternalCommand::Subscribe(payload)).await.unwrap();
        // give background task a moment to register subscriber
        tokio::time::sleep(Duration::from_millis(50)).await;

        let po = PolyMarketHistoryPo {
            instrument_id: 7,
            timestamp: unix_time_now_u64_utc(),
            price: 7.7,
        };
        poly_tx.send(po).unwrap();

        let opt = timeout(Duration::from_secs(3), client_rx.recv())
            .await
            .expect("timeout waiting for subscribe after start");
        let msg = opt.expect("stream closed").expect("status error");
        match msg.payload {
            Some(server_message::Payload::PolymarketHistory(list)) => assert_eq!(list.history_list.len(), 1),
            _ => panic!("unexpected payload"),
        }
    }
}

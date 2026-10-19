use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::models::grpc_sync::{Empty, HistoryEntry, Instrument, InstrumentList, InstrumentType, ServerMessage, SubscribeRequest, SyncRequest};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info, trace};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tonic::{Request, Response, Status};
use yue::query_message::DataSourceProviderTrait;

///
///  这里主要是为了处理在服务器端的查询抽象。主要是3层关系。
///  - YuSyncServer: 负责整体调度和与外部的接口
///  - SyncInstrumentServiceTrait: Instrument相关的Trait
///  - SyncHistoryServiceTrait：负责历史价格信息的Trait
///
/// # Instrument和exchange的关系
/// 我在试图删除交易这一个层级。主要是觉得有点累赘。因为应用下来，在历史数据这一层，更加偏向属性。
/// 所以在服务器端我就把这个给删除掉了。
///
/// # 依赖关系
/// 1. instrument和history应该是一对多，比如可能有K线，盘口信息等。
/// 2. 把最后运行的方式，用default，或者写一个for_production这类方法。
///
///

#[async_trait]
pub trait SyncInstrumentServiceTrait {
    async fn list_instruments(&self) -> Result<HashMap<String, Instrument>, YuError>;

    fn error_log(&self) -> String;

    fn support_history_list(&self) -> Result<HashMap<InstrumentType, SyncHistoryService>, YuError>;
}

#[async_trait]
pub trait SyncHistoryServiceTrait {
    ///
    /// 传入数据，并且在这里广播
    ///
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ts: u64,
        end_ts: u64,
        batch_size: usize,
        tx: Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError>;
}

pub type SyncInstrumentService = Arc<dyn SyncInstrumentServiceTrait + Send + Sync>;
pub type SyncHistoryService = Arc<dyn SyncHistoryServiceTrait + Send + Sync>;

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
    tx: Sender<Result<ServerMessage, Status>>,
}

pub struct YuSyncServer {
    batch_size: usize,
    command_channel: Sender<SyncInternalCommand>,

    //TODO: 简化扩展
    instruments_services: Vec<SyncInstrumentService>,
    history_services: HashMap<InstrumentType, SyncHistoryService>,
}

impl YuSyncServer {
    pub async fn create_and_start(
        instruments_services: Vec<SyncInstrumentService>,
        history_rx: mpsc::Receiver<HistoryEntry>,
    ) -> Result<Self, YuError> {
        let (command_channel, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        Self::start_broadcast_history(command_rx, history_rx, 500, 1000).await?;
        let mut history_services = HashMap::new();
        for service in &instruments_services {
            history_services.extend(service.support_history_list()?)
        }

        Ok(Self {
            batch_size: 500,
            command_channel,
            instruments_services,
            history_services,
        })
    }

    /// Send batched PolyMarketHistory messages to a clients.
    ///
    /// 主要的作用是把接收到的历史消息，统一发送给客户端。
    /// 因为考虑到多客户端的情况。所以在一个线程里面，统一的分发。而不是每个客户端都单独去接收广播消息。
    ///
    ///
    /// 说明（中文）:
    /// - 将从 `history_rx` 接收到的 `HistoryEntry`，按批次缓存，满足下列任一条件时把批次打包并发送给 `client_sender`：
    ///   1. 缓存条目数达到 `max_cache_size`；
    ///   2. 自上次发送后经过了 `max_loop_mill_seconds` 毫秒。
    /// - 发送时，检查对应的channel是否关闭，如果关闭，则删除出缓存的history_tx_map，并打印认知
    /// - 收到Subscribe的信息，把发布的tx，放入history_tx_map，key为id，value为tx。进行缓存
    ///
    /// 参数：
    /// - `command_rx`: 接收订阅命令的 mpsc 接收端
    /// - `history_rx`: 接收来自服务的 HistoryEntry 的 mpsc 接收端
    /// - `max_cache_size`: 达到此数量时立即触发发送
    /// - `max_loop_mill_seconds`: 达到此时间（毫秒）时触发发送

    async fn start_broadcast_history(
        mut command_rx: mpsc::Receiver<SyncInternalCommand>,
        mut history_rx: mpsc::Receiver<HistoryEntry>,
        max_cache_size: usize,
        max_loop_mill_seconds: usize,
    ) -> Result<(), YuError> {
        tokio::spawn(async move {
            info!("开启广播历史数据的任务");
            let max_cache_size = max_cache_size.max(1);
            let mut history_buffer: Vec<HistoryEntry> = Vec::with_capacity(max_cache_size);
            let mut history_tx_map: HashMap<u64, Sender<Result<ServerMessage, Status>>> = HashMap::new();
            let mut command_channel_open = true;
            let mut history_channel_open = true;
            let mut flush_interval = tokio::time::interval(Duration::from_millis(max_loop_mill_seconds.max(1) as u64));
            flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            // 第一次 tick 立即就绪；跳过它以确保按完整间隔进行首次定时刷新。
            flush_interval.tick().await;

            loop {
                let should_flush = tokio::select! {
                    command = command_rx.recv(), if command_channel_open => {
                        match command {
                            Some(SyncInternalCommand::Subscribe(payload)) => {
                                info!("Received subscribe command with id: {}", payload.id);
                                history_tx_map.insert(payload.id, payload.tx);
                            }
                            None => command_channel_open = false,
                        }
                        false
                    }
                    result = history_rx.recv(), if history_channel_open => {
                        match result {
                            Some(entry) => {
                                trace!("Received history entry with instrument type: {}", entry.instrument_type);
                                history_buffer.push(entry);
                                history_buffer.len() >= max_cache_size
                            }
                            None => {
                                info!("history channel is closed");
                                history_channel_open = false;
                                false
                            }
                        }
                    }
                    _ = flush_interval.tick() => !history_buffer.is_empty(),
                };

                if !command_channel_open && !history_channel_open {
                    break;
                }

                if should_flush && !history_buffer.is_empty() {
                    let message = ServerMessage {
                        history: std::mem::take(&mut history_buffer),
                        server_timestamp: unix_time_now_u64_utc(),
                    };
                    let mut remove_keys = Vec::new();

                    for (id, tx) in &history_tx_map {
                        if tx.is_closed() {
                            info!("Subscriber {} channel is closed, removing from list", id);
                            remove_keys.push(*id);
                            continue;
                        }

                        if let Err(e) = tx.send(Ok(message.clone())).await {
                            error!("Failed to send history to subscriber {}: {:?}", id, e);
                            remove_keys.push(*id);
                        }
                    }

                    for id in remove_keys {
                        history_tx_map.remove(&id);
                    }
                    flush_interval.reset_after(Duration::from_millis(max_loop_mill_seconds.max(1) as u64));
                }
            }
        });
        Ok(())
    }
}
#[tonic::async_trait]
impl SyncInterface for YuSyncServer {
    async fn list_instrument(&self, _request: Request<Empty>) -> Result<Response<InstrumentList>, Status> {
        let mut instruments_map: HashMap<String, Instrument> = HashMap::new();
        for svc in &self.instruments_services {
            match svc.list_instruments().await {
                Ok(instruments) => {
                    instruments_map.extend(instruments);
                }
                Err(error) => {
                    error!("{:?}:{:?}", svc.error_log(), error);
                }
            }
        }

        Ok(Response::new(InstrumentList {
            instruments: instruments_map,
            server_timestamp: unix_time_now_u64_utc(),
        }))
    }

    type SyncHistoryStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<ServerMessage, Status>> + Send + 'static>>;

    ///
    /// 主要流程
    /// 1. 通过请求的instrument_type获取对应的history_service
    /// 2. 开启一个线程，来调用相应的方法
    ///
    async fn sync_history(&self, request: Request<SyncRequest>) -> Result<Response<Self::SyncHistoryStream>, Status> {
        let (tx, rx) = mpsc::channel::<Result<ServerMessage, Status>>(16);
        let inst_id = request.get_ref().inst_id.clone();
        let start_ms = request.get_ref().start_ms.clone();
        let end_ms = request.get_ref().end_ms.clone();
        let batch_size = self.batch_size.clone();

        let instrument_type = match InstrumentType::try_from(request.get_ref().instrument_type) {
            Ok(instrument_type) => instrument_type,
            Err(_) => {
                error!("unknown instrument type:{:?}", request.get_ref().instrument_type);
                return Err(Status::invalid_argument("unknown instrument type"));
            }
        };

        let query_service = match self.history_services.get(&instrument_type) {
            Some(service) => service.clone(),
            None => {
                let inst_type_name = instrument_type.readable_name();
                error!("instrument type not configure service:type is {}", inst_type_name);
                return Err(Status::not_found("history service is not configured"));
            }
        };

        tokio::spawn(async move {
            if let Err(e) = query_service.query_and_broadcast_history(inst_id, start_ms, end_ms, batch_size, tx).await {
                let inst_type_name = instrument_type.readable_name();
                error!(
                    "error when query instrument history: inst_type is {}, id is {}, error is {:?}",
                    inst_type_name, inst_id, e
                );
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
    use crate::sync::models::grpc_sync::history_entry;
    use async_trait::async_trait;
    use futures::StreamExt;
    use tokio::time::{Duration, timeout};

    fn test_history_entry(inst_id: u64) -> HistoryEntry {
        HistoryEntry {
            instrument_type: InstrumentType::PolymarketToken as i32,
            payload: Some(history_entry::Payload::PolymarketHistory(
                crate::sync::models::grpc_sync::PolyMarketHistory {
                    inst_id,
                    timestamp: inst_id * 10,
                    price: inst_id as f64,
                },
            )),
        }
    }

    struct RecordingHistoryService {
        calls: std::sync::Mutex<Vec<(u64, u64, u64, usize)>>,
    }

    #[async_trait]
    impl SyncHistoryServiceTrait for RecordingHistoryService {
        async fn query_and_broadcast_history(
            &self,
            inst_id: u64,
            start_ts: u64,
            end_ts: u64,
            batch_size: usize,
            tx: Sender<Result<ServerMessage, Status>>,
        ) -> Result<(), YuError> {
            self.calls.lock().unwrap().push((inst_id, start_ts, end_ts, batch_size));
            tx.send(Ok(ServerMessage {
                history: Vec::new(),
                server_timestamp: 0,
            }))
            .await
            .expect("send history message");
            Ok(())
        }
    }

    fn test_sync_server(history_services: HashMap<InstrumentType, SyncHistoryService>) -> YuSyncServer {
        let (command_channel, _) = mpsc::channel(1);
        YuSyncServer {
            batch_size: 8,
            command_channel,
            instruments_services: Vec::new(),
            history_services,
        }
    }

    #[tokio::test]
    async fn sync_history_rejects_unknown_instrument_type() {
        // 测试准备：创建不包含任何历史服务的同步服务器。
        let server = test_sync_server(HashMap::new());

        // 测试步骤：请求一个未定义的 instrument_type。
        let result = server
            .sync_history(Request::new(SyncRequest {
                inst_id: 1,
                start_ms: 10,
                end_ms: 20,
                instrument_type: 999,
            }))
            .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("unknown instrument type should be rejected"),
        };

        // 测试目的：确认非法 instrument_type 返回 InvalidArgument。
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn sync_history_returns_not_found_when_service_is_unconfigured() {
        // 测试准备：创建未注册 OKX K 线历史服务的同步服务器。
        let server = test_sync_server(HashMap::new());

        // 测试步骤：使用合法但未配置服务的 instrument_type 发起请求。
        let result = server
            .sync_history(Request::new(SyncRequest {
                inst_id: 1,
                start_ms: 10,
                end_ms: 20,
                instrument_type: InstrumentType::OkxKline as i32,
            }))
            .await;
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("unconfigured history service should be rejected"),
        };

        // 测试目的：确认服务未配置时返回 NotFound。
        assert_eq!(error.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn sync_history_forwards_request_and_streams_service_messages() {
        // 测试准备：注册可记录请求参数并返回一条消息的模拟历史服务。
        let history_service = Arc::new(RecordingHistoryService {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let mut history_services = HashMap::new();
        history_services.insert(InstrumentType::BinanceSpot, history_service.clone() as SyncHistoryService);
        let server = test_sync_server(history_services);

        // 测试步骤：请求 Binance Spot 历史数据，并从响应流中读取服务返回的消息。
        let response = server
            .sync_history(Request::new(SyncRequest {
                inst_id: 42,
                start_ms: 100,
                end_ms: 200,
                instrument_type: InstrumentType::BinanceSpot as i32,
            }))
            .await
            .unwrap();
        let mut stream = response.into_inner();

        let message = stream.next().await.unwrap().unwrap();
        assert!(message.history.is_empty());
        assert!(stream.next().await.is_none());

        // 测试目的：确认请求参数被完整转发，服务消息进入响应流且流正常结束。
        let actual = history_service.calls.lock().unwrap().clone();
        assert_eq!(actual.len(), 1);
        let (inst_id, start_ts, end_ts, batch_size) = actual[0];
        assert_eq!(inst_id, 42);
        assert_eq!(start_ts, 100);
        assert_eq!(end_ts, 200);
        assert_eq!(batch_size, 8);
    }

    #[tokio::test]
    async fn start_broadcast_history_flushes_when_batch_size_is_reached() {
        // 测试准备：启动广播任务，配置批量大小为 2，并建立一个订阅者。
        let (command_tx, command_rx) = mpsc::channel(4);
        let (history_tx, history_rx) = mpsc::channel(4);
        let (client_tx, mut client_rx) = mpsc::channel(4);
        YuSyncServer::start_broadcast_history(command_rx, history_rx, 2, 60_000).await.unwrap();
        command_tx
            .send(SyncInternalCommand::Subscribe(SubscribePayload { id: 1, tx: client_tx }))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        // 测试步骤：发送两条历史数据，等待客户端收到同一个批次。
        history_tx.send(test_history_entry(11)).await.unwrap();
        history_tx.send(test_history_entry(12)).await.unwrap();
        let message = timeout(Duration::from_secs(1), client_rx.recv()).await.unwrap().unwrap().unwrap();

        // 测试目的：确认缓存达到阈值时立即刷新，且批次保留全部历史条目。
        assert_eq!(message.history.len(), 2);
        assert_eq!(message.history[0].instrument_type, InstrumentType::PolymarketToken as i32);
        assert_eq!(message.history[1].instrument_type, InstrumentType::PolymarketToken as i32);
        assert_ne!(message.server_timestamp, 0);
    }

    #[tokio::test]
    async fn start_broadcast_history_flushes_after_interval() {
        // 测试准备：启动广播任务，配置远高于输入数量的批量阈值，并建立一个订阅者。
        let (command_tx, command_rx) = mpsc::channel(4);
        let (history_tx, history_rx) = mpsc::channel(4);
        let (client_tx, mut client_rx) = mpsc::channel(4);
        YuSyncServer::start_broadcast_history(command_rx, history_rx, 10, 30).await.unwrap();
        command_tx
            .send(SyncInternalCommand::Subscribe(SubscribePayload { id: 2, tx: client_tx }))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        // 测试步骤：只发送一条未达到批量阈值的历史数据，等待定时刷新。
        history_tx.send(test_history_entry(21)).await.unwrap();
        let message = timeout(Duration::from_secs(1), client_rx.recv()).await.unwrap().unwrap().unwrap();

        // 测试目的：确认缓存未满时也会按配置时间发送。
        assert_eq!(message.history.len(), 1);
        assert_eq!(message.history[0].instrument_type, InstrumentType::PolymarketToken as i32);
    }

    #[tokio::test]
    async fn start_broadcast_history_removes_closed_subscribers() {
        // 测试准备：启动广播任务并注册一个已关闭和一个正常的客户端 channel。
        let (command_tx, command_rx) = mpsc::channel(4);
        let (history_tx, history_rx) = mpsc::channel(4);
        let (closed_client_tx, closed_client_rx) = mpsc::channel(1);
        let (active_client_tx, mut active_client_rx) = mpsc::channel(4);
        YuSyncServer::start_broadcast_history(command_rx, history_rx, 1, 60_000).await.unwrap();
        command_tx
            .send(SyncInternalCommand::Subscribe(SubscribePayload { id: 3, tx: closed_client_tx }))
            .await
            .unwrap();
        command_tx
            .send(SyncInternalCommand::Subscribe(SubscribePayload { id: 4, tx: active_client_tx }))
            .await
            .unwrap();
        drop(closed_client_rx);
        tokio::time::sleep(Duration::from_millis(20)).await;

        // 测试步骤：触发一次刷新，再发送第二条数据确认广播任务仍正常处理订阅者。
        history_tx.send(test_history_entry(31)).await.unwrap();
        let first = timeout(Duration::from_secs(1), active_client_rx.recv()).await.unwrap().unwrap().unwrap();
        history_tx.send(test_history_entry(32)).await.unwrap();
        let second = timeout(Duration::from_secs(1), active_client_rx.recv()).await.unwrap().unwrap().unwrap();

        // 测试目的：确认关闭的客户端不会阻断其他客户端接收后续广播。
        assert_eq!(first.history.len(), 1);
        assert_eq!(second.history.len(), 1);
    }
}

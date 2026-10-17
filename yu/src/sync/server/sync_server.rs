use crate::duck_db::DuckDBDSProvider;
use crate::errors::YuError;
use crate::okx::duck_po::{OkxKlinePo, OkxOptionSummaryPo};
use crate::polymarket::po::PolyMarketHistoryPo;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::models::grpc_sync::{
    Empty, ExchangeType, Instrument, InstrumentList, InstrumentType, OkxKline, PolyMarketHistory, PolyMarketHistoryList, ServerMessage,
    SubscribeRequest, SyncRequest, server_message,
};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc;
use log::{error, info, trace};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{broadcast, mpsc};
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
    pub async fn create_and_start(instruments_services: Vec<SyncInstrumentService>) -> Result<Self, YuError> {
        let (command_channel, _) = mpsc::channel::<SyncInternalCommand>(100);

        // let polymarket_history_receiver = polymarket_history_service.subscribe_history_broadcast();
        // let okx_option_kline_receiver = okx_option_service.subscribe_kline().await;
        // let okx_option_summary_receiver = okx_option_service.subscribe_option_summary().await;
        // Self::start_broadcast_history(
        //     command_rx,
        //     polymarket_history_receiver,
        //     okx_option_kline_receiver,
        //     okx_option_summary_receiver,
        //     500,
        //     1000,
        // )
        // .await?;
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
        mut okx_option_kline_receiver: broadcast::Receiver<OkxKlinePo>,
        mut okx_option_summary_receiver: broadcast::Receiver<OkxOptionSummaryPo>,
        max_cache_size: usize,
        max_loop_mill_seconds: usize,
    ) -> Result<(), YuError> {
        tokio::spawn(async move {
            info!("开启广播历史数据的任务");
            // 临时缓存单条 history（proto）用于批量发送
            let mut pm_batch_buffer: Vec<PolyMarketHistory> = Vec::new();
            // 为 okx kline 增加缓存，行为与 polymarket 缓存一致
            let mut okx_batch_buffer: Vec<OkxKline> = Vec::new();
            let mut last_broadcast_timestamp: u64 = unix_time_now_u64_utc();
            let mut history_tx_map: HashMap<u64, Sender<Result<ServerMessage, Status>>> = HashMap::new();

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
                                pm_batch_buffer.push(item);
                            }
                            Err(e) => {
                                error!("history broadcast receiver error: {:?}", e);
                                // 如果没有活跃发送者，短暂休眠再试
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    recv_okx = okx_option_kline_receiver.recv() => {
                        match recv_okx {
                            Ok(po) => {
                                trace!("Received okx kline broadcast inst_id: {} ts: {}", po.inst_id, po.ts);
                                // 转成 proto 并缓存，按批次发送（与 polymarket 行为一致）
                                let k =OkxKline {
                                    id: po.id,
                                    inst_id: po.inst_id,
                                    ts: po.ts,
                                    open: po.open,
                                    high: po.high,
                                    low: po.low,
                                    close: po.close,
                                    vol: po.vol,
                                    vol_ccy: po.vol_ccy,
                                    vol_ccy_quote: po.vol_ccy_quote,
                                    confirm: po.confirm as u32,
                                };
                                okx_batch_buffer.push(k);
                            }
                            Err(e) => {
                                error!("okx kline broadcast receiver error: {:?}", e);
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    recv_summary = okx_option_summary_receiver.recv() => {
                        match recv_summary {
                            Ok(po) => {
                                trace!("Received okx option summary broadcast inst_id: {} server_ts: {}", po.inst_id, po.server_ts);
                                let summary = crate::sync::models::grpc_sync::OkxOptionSummary {
                                    id: po.id,
                                    inst_id: po.inst_id,
                                    inst_identify: po.inst_identify,
                                    inst_type: po.inst_type,
                                    uly: po.uly.unwrap_or_default(),
                                    acquire_ts: po.acquire_ts,
                                    server_ts: po.server_ts,
                                    ask_vol: po.ask_vol,
                                    bid_vol: po.bid_vol,
                                    delta: po.delta,
                                    delta_bs: po.delta_bs,
                                    fwd_px: po.fwd_px,
                                    gamma: po.gamma,
                                    gamma_bs: po.gamma_bs,
                                    lever: po.lever,
                                    mark_vol: po.mark_vol,
                                    real_vol: po.real_vol,
                                    vol_lv: po.vol_lv,
                                    theta: po.theta,
                                    theta_bs: po.theta_bs,
                                    vega: po.vega,
                                    vega_bs: po.vega_bs,
                                };
                                // 使用单独缓存，便于和 kline / polymarket 分支并行汇总发送
                                let mut merged = Vec::new();
                                merged.push(summary);
                                let list = crate::sync::models::grpc_sync::OkxOptionSummaryList {
                                    summary_list: merged,
                                    timestamp: unix_time_now_u64_utc(),
                                };
                                let msg = ServerMessage {
                                    payload: Some(server_message::Payload::OptionSummaryHistory(list)),
                                };
                                for (id, tx) in history_tx_map.iter_mut() {
                                    if tx.is_closed() {
                                        continue;
                                    }
                                    if let Err(e) = tx.send(Ok(msg.clone())).await {
                                        error!("Failed to send okx option summary to subscriber {}: {:?}", id, e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("okx option summary broadcast receiver error: {:?}", e);
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

                // 检查是否需要触发发送：缓存大小或时间间隔（对 polymarket 和 okx 都适用）
                let now_ms = unix_time_now_u64_utc();
                let elapsed_ms = now_ms.saturating_sub(last_broadcast_timestamp);
                let poly_needs = !pm_batch_buffer.is_empty() && (pm_batch_buffer.len() >= max_cache_size);
                let okx_needs = !okx_batch_buffer.is_empty() && (okx_batch_buffer.len() >= max_cache_size);
                let time_needs = elapsed_ms as usize >= max_loop_mill_seconds && (!pm_batch_buffer.is_empty() || !okx_batch_buffer.is_empty());

                if poly_needs || okx_needs || time_needs {
                    // 准备要发送的消息（如果对应缓存非空）
                    let poly_msg_opt = if !pm_batch_buffer.is_empty() {
                        let list = PolyMarketHistoryList {
                            history_list: pm_batch_buffer.clone(),
                            timestamp: unix_time_now_u64_utc(),
                        };
                        Some(ServerMessage {
                            payload: Some(server_message::Payload::PolymarketHistory(list)),
                        })
                    } else {
                        None
                    };

                    let okx_msg_opt = if !okx_batch_buffer.is_empty() {
                        let list = crate::sync::models::grpc_sync::OkxKlineList {
                            kline_list: okx_batch_buffer.clone(),
                            timestamp: unix_time_now_u64_utc(),
                        };
                        Some(ServerMessage {
                            payload: Some(server_message::Payload::OkxKlineHistory(list)),
                        })
                    } else {
                        None
                    };

                    // 发送给所有订阅者，检测并清理已关闭的channel
                    let mut remove_keys: Vec<u64> = Vec::new();
                    for (id, tx) in history_tx_map.iter_mut() {
                        if tx.is_closed() {
                            info!("Subscriber {} channel is closed, removing from list", id);
                            remove_keys.push(*id);
                            continue;
                        }

                        if let Some(ref msg) = poly_msg_opt {
                            let send_res = tx.send(Ok(msg.clone())).await;
                            if let Err(e) = send_res {
                                error!("Failed to send history to subscriber {}: {:?}", id, e);
                                remove_keys.push(*id);
                                continue; // if this tx failed, skip okx send for this id
                            }
                        }

                        if let Some(ref msg) = okx_msg_opt {
                            let send_res = tx.send(Ok(msg.clone())).await;
                            if let Err(e) = send_res {
                                error!("Failed to send okx kline to subscriber {}: {:?}", id, e);
                                remove_keys.push(*id);
                            }
                        }
                    }

                    for k in remove_keys {
                        history_tx_map.remove(&k);
                    }

                    // 更新状态并清空缓存
                    last_broadcast_timestamp = now_ms;
                    pm_batch_buffer.clear();
                    okx_batch_buffer.clear();
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
    use async_trait::async_trait;
    use futures::StreamExt;
    use tokio::time::{Duration, timeout};

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
            tx.send(Ok(ServerMessage { payload: None })).await.expect("send history message");
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
        let server = test_sync_server(HashMap::new());

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

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn sync_history_returns_not_found_when_service_is_unconfigured() {
        let server = test_sync_server(HashMap::new());

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

        assert_eq!(error.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn sync_history_forwards_request_and_streams_service_messages() {
        let history_service = Arc::new(RecordingHistoryService {
            calls: std::sync::Mutex::new(Vec::new()),
        });
        let mut history_services = HashMap::new();
        history_services.insert(InstrumentType::BinanceSpot, history_service.clone() as SyncHistoryService);
        let server = test_sync_server(history_services);

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
        assert!(message.payload.is_none());
        assert!(stream.next().await.is_none());
        let actual = history_service.calls.lock().unwrap().clone();
        assert_eq!(actual.len(), 1);
        let (inst_id, start_ts, end_ts, batch_size) = actual[0];
        assert_eq!(inst_id, 42);
        assert_eq!(start_ts, 100);
        assert_eq!(end_ts, 200);
        assert_eq!(batch_size, 8);
    }

    // 1) 缓存达到大小时立即发送
    #[tokio::test]
    async fn test_batch_send_on_size() {
        let (command_tx, command_rx) = mpsc::channel::<SyncInternalCommand>(100);
        let (poly_tx, poly_rx) = broadcast::channel::<PolyMarketHistoryPo>(16);
        let (_okx_tx, okx_rx) = broadcast::channel::<OkxKlinePo>(16);
        let (_okx_option_tx, okx_option_rx) = broadcast::channel::<OkxOptionSummaryPo>(16);

        // spawn the broadcaster
        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, okx_rx, okx_option_rx, 3, 10_000)
            .await
            .unwrap();

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
        let (_okx_tx, okx_rx) = broadcast::channel::<OkxKlinePo>(16);
        let (_okx_option_tx, okx_option_rx) = broadcast::channel::<OkxOptionSummaryPo>(16);

        // use small time threshold (ms)
        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, okx_rx, okx_option_rx, 10, 800)
            .await
            .unwrap();

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
        let (_okx_tx, okx_rx) = broadcast::channel::<OkxKlinePo>(16);
        let (_okx_option_tx, okx_option_rx) = broadcast::channel::<OkxOptionSummaryPo>(16);

        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, okx_rx, okx_option_rx, 1, 10_000)
            .await
            .unwrap();

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
        let (_okx_tx, okx_rx) = broadcast::channel::<OkxKlinePo>(16);
        let (_okx_option_tx, okx_option_rx) = broadcast::channel::<OkxOptionSummaryPo>(16);

        let _ = YuSyncServer::start_broadcast_history(command_rx, poly_rx, okx_rx, okx_option_rx, 1, 10_000)
            .await
            .unwrap();

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

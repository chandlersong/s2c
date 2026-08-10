use crate::duck_db::DuckDBDSProvider;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::sync_interface_server::SyncInterface;
use crate::sync::models::grpc_sync::{
    Empty, InstrumentList, PolyMarketHistory, PolyMarketHistoryList, PolymarketInstrument, ServerMessage, SubscribeRequest, SyncRequest,
    server_message,
};
use li::tools::time::unix_time_now_u64_utc;
use log::error;
use std::collections::HashMap;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};
use tonic::{Request, Response, Status};
use yue::okx::models::common::InstrumentInfo;
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
    QueryAssetTimestamp(oneshot::Sender<Result<InstrumentInfo, Status>>),
}

pub struct YuSyncServer {
    polymarket_history_service: SeriesHistoryMarketService,
    batch_size: usize,
}

impl YuSyncServer {
    pub async fn new(polymarket_history_service: SeriesHistoryMarketService) -> Self {
        Self {
            polymarket_history_service,
            batch_size: 500,
        }
    }
}
#[tonic::async_trait]
impl SyncInterface for YuSyncServer {
    async fn list_instrument(&self, request: Request<Empty>) -> Result<Response<InstrumentList>, Status> {
        let polymarket_instruments = self.polymarket_history_service.list_instruments().await;

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

                Ok(Response::new(InstrumentList {
                    instruments: instruments_map,
                }))
            }
            Err(e) => {
                error!("list_instruments error: {:?}", e);
                Err(Status::internal(format!("list_instruments error: {:?}", e)))
            }
        }
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
        todo!()
    }
}

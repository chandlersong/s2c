use crate::errors::YuError;
use crate::polymarket::service::{SeriesHistoryMarketService, SeriesHistoryMarketServiceTrait};
use crate::sync::models::grpc_sync::{
    ExchangeType, Instrument, InstrumentType, PolyMarketHistory, PolyMarketHistoryList, PolymarketInstrument, ServerMessage, server_message,
};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc;
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tonic::Status;

pub struct PolyMarketSyncInstrumentService {
    polymarket_history_service: SeriesHistoryMarketService,
    support_history_service: HashMap<InstrumentType, SyncHistoryService>,
}

impl PolyMarketSyncInstrumentService {
    pub fn new(
        polymarket_history_service: SeriesHistoryMarketService,
        support_history_service: HashMap<InstrumentType, SyncHistoryService>,
    ) -> SyncInstrumentService {
        Arc::new(Self {
            polymarket_history_service,
            support_history_service,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for PolyMarketSyncInstrumentService {
    async fn list_instruments(&self) -> Result<HashMap<String, Instrument>, YuError> {
        let polymarket_instruments = self.polymarket_history_service.list_instruments().await;
        let mut instruments_map: HashMap<String, Instrument> = HashMap::new();
        match polymarket_instruments {
            Ok(list) => {
                for inst in list {
                    // 直接把 PolyMarketInstrumentPo 转换为 proto，latest_timestamp 暂时置为 0
                    let asset_id = inst.asset_id.clone();
                    let poly = PolymarketInstrument::from(inst);

                    let instrument = Instrument {
                        exchange: ExchangeType::Polymarket as i32,
                        payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Polymarket(poly)),
                    };
                    instruments_map.insert(asset_id, instrument);
                }
            }
            Err(e) => {
                error!("list_instruments error: {:?}", e);
                return Err(Status::internal(format!("list polymarket instruments error: {:?}", e)).into());
            }
        };
        Ok(instruments_map)
    }

    fn error_log(&self) -> String {
        "error when list polymarket instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<InstrumentType, SyncHistoryService>, YuError> {
        Ok(self.support_history_service.clone())
    }
}

pub struct PolyMarketSyncHistoryService {
    polymarket_history_service: SeriesHistoryMarketService,
}

impl PolyMarketSyncHistoryService {
    pub fn new(polymarket_history_service: SeriesHistoryMarketService) -> SyncHistoryService {
        Arc::new(Self { polymarket_history_service })
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for PolyMarketSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ms: u64,
        end_ms: u64,
        batch_size: usize,
        tx: Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        match self.polymarket_history_service.query_instrument_history(inst_id, start_ms, end_ms).await {
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
                    return Ok(());
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
                Ok(())
            }
            Err(e) => {
                let status = Status::internal(format!("query history error: {:?}", e));
                let _ = tx.send(Err(status.clone())).await;
                Err(status.into())
            }
        }
    }
}

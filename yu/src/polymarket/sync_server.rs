use crate::errors::YuError;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::{ExchangeType, Instrument, PolymarketInstrument};
use crate::sync::server::sync_server::{SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Status;

pub struct PolyMarketSyncInstrumentService {
    polymarket_history_service: SeriesHistoryMarketService,
}

impl PolyMarketSyncInstrumentService {
    pub fn new(polymarket_history_service: SeriesHistoryMarketService) -> SyncInstrumentService {
        Arc::new(Self { polymarket_history_service })
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
}

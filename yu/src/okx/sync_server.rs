use crate::errors::YuError;
use crate::okx::service::OptionService;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::{Instrument, PolymarketInstrument};
use crate::sync::server::sync_server::{SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Status;

pub struct OkxSyncInstrumentService {
    okx_option_service: Arc<OptionService>,
}

impl OkxSyncInstrumentService {
    pub fn new(okx_option_service: Arc<OptionService>) -> SyncInstrumentService {
        Arc::new(Self { okx_option_service })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for OkxSyncInstrumentService {
    async fn list_instruments(&self) -> Result<HashMap<String, Instrument>, YuError> {
        let mut instruments_map: HashMap<String, Instrument> = HashMap::new();
        let okx_option_instruments = self.okx_option_service.list_instruments().await;
        match okx_option_instruments {
            Ok(okx_list) => {
                for inst in okx_list {
                    let key = inst.inst_identify.clone();
                    let okx = crate::sync::models::grpc_sync::OkxInstrument::from(inst);
                    let instrument = Instrument {
                        payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Okx(okx)),
                    };
                    instruments_map.insert(key, instrument);
                }
            }
            Err(e) => {
                error!("list_okx_instruments error: {:?}", e);
                return Err(Status::internal(format!("list okx option instruments error: {:?}", e)).into());
            }
        };
        Ok(instruments_map)
    }

    fn error_log(&self) -> String {
        "error when list okx instruments".to_string()
    }
}

use crate::errors::YuError;
use crate::okx::duck_po::{InstrumentPo, OkxKlinePo};
use crate::okx::service::{OkxRemoteService, OptionService, okx_remote_service_for_production};
use crate::sync::models::grpc_sync::{HistoryEntry, HistoryType, Instrument, InstrumentType, OkxInstrument, OkxKline, ServerMessage, history_entry};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use log::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tonic::Status;
use yue::okx::restful_api::InstrumentsParam;

fn history_entry_from_kline(kline: &OkxKlinePo) -> HistoryEntry {
    HistoryEntry {
        history_type: HistoryType::OkxKline.into(),
        instrument_type: InstrumentType::Okx.into(),
        inst_id: kline.inst_id,
        payload: Some(history_entry::Payload::OkxKline(OkxKline {
            id: kline.id,
            ts: kline.ts,
            open: kline.open,
            high: kline.high,
            low: kline.low,
            close: kline.close,
            vol: kline.vol,
            vol_ccy: kline.vol_ccy,
            vol_ccy_quote: kline.vol_ccy_quote,
            confirm: kline.confirm.into(),
        })),
    }
}

///
/// 当前版本，只是处理option的Instrument
pub struct OkxSyncInstrumentService {
    ok_remote: OkxRemoteService,
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
    live_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
}

impl OkxSyncInstrumentService {
    pub fn for_production(
        support_history_service: HashMap<HistoryType, SyncHistoryService>,
        live_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            ok_remote: okx_remote_service_for_production(),
            support_history_service,
            live_instruments,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for OkxSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        let instruments = self
            .ok_remote
            .get_instrument_repo()
            .get_instrument_by_type(yue::models::InstrumentType::Option)
            .await?;

        Ok(instruments
            .into_iter()
            .map(|instrument| {
                let inst_id = instrument.id.to_string();
                let instrument = Instrument {
                    instrument_type: InstrumentType::Okx as i32,
                    payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Okx(OkxInstrument::from(instrument))),
                };
                instrument
            })
            .collect())
    }

    async fn sync_instrument(&self) -> Result<Vec<Instrument>, YuError> {
        // call for BTC and ETH
        self.ok_remote
            .sync_instruments(InstrumentsParam::query_option("BTC-USD"), yue::models::InstrumentType::Option)
            .await?;
        self.ok_remote
            .sync_instruments(InstrumentsParam::query_option("ETH-USD"), yue::models::InstrumentType::Option)
            .await?;
        let live_instruments = self
            .ok_remote
            .get_instrument_repo()
            .get_instrument_by_type_live(yue::models::InstrumentType::Option)
            .await?;
        {
            let mut guard = self.live_instruments.write().await;
            *guard = live_instruments.clone();
        }
        info!("synced OKX instruments, total count: {}", live_instruments.len());
        Ok(live_instruments
            .into_iter()
            .map(|instrument| Instrument {
                instrument_type: InstrumentType::Okx as i32,
                payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Okx(OkxInstrument::from(instrument))),
            })
            .collect())
    }

    fn error_log(&self) -> String {
        "error when list okx instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<HistoryType, SyncHistoryService>, YuError> {
        Ok(self.support_history_service.clone())
    }
}

pub struct OkxSyncHistoryService {
    okx_option_service: Arc<OptionService>,
    history_tx: mpsc::Sender<HistoryEntry>,
}

impl OkxSyncHistoryService {
    pub fn for_production(okx_option_service: Arc<OptionService>, history_tx: mpsc::Sender<HistoryEntry>) -> Arc<Self> {
        Arc::new(Self {
            okx_option_service,
            history_tx,
        })
    }

    pub async fn broadcast_message(&self, kline: OkxKlinePo) {
        let history_entry = history_entry_from_kline(&kline);
        if let Err(error) = self.history_tx.send(history_entry).await {
            error!(
                "failed to broadcast OKX kline, inst_id:{}, timestamp:{}, error:{}",
                kline.inst_id, kline.ts, error
            );
        }
    }

    pub async fn check_and_broadcast_history(&self) -> Result<(), YuError> {
        for kline in self.okx_option_service.check_history_data().await? {
            self.broadcast_message(kline).await;
        }
        Ok(())
    }

    pub async fn sync_all_history(&self) -> Result<(), YuError> {
        for kline in self.okx_option_service.sync_latest_candles().await? {
            self.broadcast_message(kline).await;
        }
        Ok(())
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for OkxSyncHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ms: u64,
        end_ms: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        let history = self.okx_option_service.find_candle_between(inst_id, start_ms, end_ms).await;
        match history {
            Ok(history) if history.is_empty() => {
                let message = ServerMessage {
                    history: vec![],
                    server_timestamp: li::tools::time::unix_time_now_u64_utc(),
                };
                if tx.send(Ok(message)).await.is_err() {
                    return Ok(());
                }
                Ok(())
            }
            Ok(history) => {
                for batch in history.chunks(batch_size.max(1)) {
                    let entries = batch.iter().map(history_entry_from_kline).collect();
                    let message = ServerMessage {
                        history: entries,
                        server_timestamp: li::tools::time::unix_time_now_u64_utc(),
                    };
                    if tx.send(Ok(message)).await.is_err() {
                        break;
                    }
                }
                Ok(())
            }
            Err(error) => {
                error!("query OKX kline history failed: {:?}", error);
                let status = Status::internal(format!("query OKX kline history failed: {error}"));
                let _ = tx.send(Err(status.clone())).await;
                Err(status.into())
            }
        }
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        self.check_and_broadcast_history().await
    }

    async fn initial_history_data(&self) -> Result<(), YuError> {
        self.okx_option_service.initial_candle(0, None).await
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        self.sync_all_history().await
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_kline_to_sync_history_entry() {
        let kline = OkxKlinePo {
            id: 11,
            inst_id: 7,
            ts: 1_700_000_000_000,
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            vol: 3.0,
            vol_ccy: 4.0,
            vol_ccy_quote: 5.0,
            confirm: 1,
        };

        let entry = history_entry_from_kline(&kline);

        assert_eq!(HistoryType::try_from(entry.history_type), Ok(HistoryType::OkxKline));
        assert_eq!(InstrumentType::try_from(entry.instrument_type), Ok(InstrumentType::Okx));
        assert_eq!(entry.inst_id, 7);
        let Some(history_entry::Payload::OkxKline(proto)) = entry.payload else {
            panic!("expected OKX Kline payload");
        };
        assert_eq!(proto.id, kline.id);
        assert_eq!(proto.ts, kline.ts);
        assert_eq!(proto.open, kline.open);
        assert_eq!(proto.high, kline.high);
        assert_eq!(proto.low, kline.low);
        assert_eq!(proto.close, kline.close);
        assert_eq!(proto.vol, kline.vol);
        assert_eq!(proto.vol_ccy, kline.vol_ccy);
        assert_eq!(proto.vol_ccy_quote, kline.vol_ccy_quote);
        assert_eq!(proto.confirm, u32::from(kline.confirm));
    }
}

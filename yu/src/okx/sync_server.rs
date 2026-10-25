use crate::errors::YuError;
use crate::okx::duck_po::{InstrumentPo, OkxKlinePo, OkxOptionSummaryPo};
use crate::okx::duckdb_repository::{OkxKlineRepository, OkxOptionSummaryRepository};
use crate::okx::service::{OkxSyncService, OptionService, okx_sync_service_for_production};
use crate::sync::models::grpc_sync::{
    HistoryEntry, HistoryType, Instrument, InstrumentType, OkxInstrument, OkxKline, OkxOptionSummary, ServerMessage, history_entry,
};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::websocket::connection::MessageHandlerTrait;
use log::{error, info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use tonic::Status;
use yue::okx::models::websocket::OkxWebsocketResponse;
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

fn history_entry_from_option_summary(summary: &OkxOptionSummaryPo) -> HistoryEntry {
    HistoryEntry {
        history_type: HistoryType::OkxOptionSummary.into(),
        instrument_type: InstrumentType::Okx.into(),
        inst_id: summary.inst_id,
        payload: Some(history_entry::Payload::OkxOptionSummary(OkxOptionSummary {
            id: summary.id,
            inst_identify: summary.inst_identify.clone(),
            inst_type: summary.inst_type.clone(),
            uly: summary.uly.clone().unwrap_or_default(),
            acquire_ts: summary.acquire_ts,
            server_ts: summary.server_ts,
            ask_vol: summary.ask_vol,
            bid_vol: summary.bid_vol,
            delta: summary.delta,
            delta_bs: summary.delta_bs,
            fwd_px: summary.fwd_px,
            gamma: summary.gamma,
            gamma_bs: summary.gamma_bs,
            lever: summary.lever,
            mark_vol: summary.mark_vol,
            real_vol: summary.real_vol,
            vol_lv: summary.vol_lv,
            theta: summary.theta,
            theta_bs: summary.theta_bs,
            vega: summary.vega,
            vega_bs: summary.vega_bs,
        })),
    }
}

///
/// 当前版本，只是处理option的Instrument
pub struct OkxSyncInstrumentService {
    okx_sync: OkxSyncService,
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
    live_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
}

impl OkxSyncInstrumentService {
    pub fn for_production(
        support_history_service: HashMap<HistoryType, SyncHistoryService>,
        live_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            okx_sync: okx_sync_service_for_production(),
            support_history_service,
            live_instruments,
        })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for OkxSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        let instruments = self
            .okx_sync
            .get_instrument_repo()
            .get_instrument_by_type(yue::models::InstrumentType::Option)
            .await?;

        Ok(instruments
            .into_iter()
            .map(|instrument| {
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
        self.okx_sync
            .sync_instruments(InstrumentsParam::query_option("BTC-USD"), yue::models::InstrumentType::Option)
            .await?;
        self.okx_sync
            .sync_instruments(InstrumentsParam::query_option("ETH-USD"), yue::models::InstrumentType::Option)
            .await?;
        let live_instruments = self
            .okx_sync
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

pub struct OkxSyncKlineHandler {
    kline_repo: OkxKlineRepository,
    history_tx: mpsc::Sender<HistoryEntry>,
    live_instruments: Arc<RwLock<Vec<InstrumentPo>>>,
    instrument_id_cache: RwLock<HashMap<String, u64>>,
}

impl OkxSyncKlineHandler {
    pub fn new(kline_repo: OkxKlineRepository, history_tx: mpsc::Sender<HistoryEntry>, live_instruments: Arc<RwLock<Vec<InstrumentPo>>>) -> Self {
        Self {
            kline_repo,
            history_tx,
            live_instruments,
            instrument_id_cache: RwLock::new(HashMap::new()),
        }
    }

    async fn resolve_inst_id(&self, inst_identify: &str) -> Option<u64> {
        if let Some(inst_id) = self.instrument_id_cache.read().await.get(inst_identify).copied() {
            return Some(inst_id);
        }

        let inst_id = {
            let instruments = self.live_instruments.read().await;
            instruments
                .iter()
                .find(|instrument| instrument.inst_identify == inst_identify)
                .map(|instrument| instrument.id)
        };
        if let Some(inst_id) = inst_id {
            self.instrument_id_cache.write().await.entry(inst_identify.to_string()).or_insert(inst_id);
        }
        inst_id
    }
}

#[async_trait]
impl MessageHandlerTrait<OkxWebsocketResponse> for OkxSyncKlineHandler {
    async fn handle_message(&self, message: &OkxWebsocketResponse) {
        match message {
            OkxWebsocketResponse::SubscribeResponse(response) => {
                if response.event == "error" {
                    warn!("OKX kline subscription failed: {:?}", response.msg);
                } else {
                    trace!("OKX kline subscription response: {:?}", response);
                }
            }
            OkxWebsocketResponse::Kline(payload) => {
                let mut confirmed_payload = payload.clone();
                confirmed_payload
                    .data
                    .retain(|candle| candle.last().is_some_and(|confirm| confirm == "1"));
                if confirmed_payload.data.is_empty() {
                    return;
                }

                let Some(inst_id) = self.resolve_inst_id(&payload.arg.inst_id).await else {
                    warn!("Received OKX kline for non-live instrument: {}", payload.arg.inst_id);
                    return;
                };

                for kline in OkxKlinePo::from_ws_response(inst_id, &confirmed_payload) {
                    if let Err(error) = self.kline_repo.insert_history(kline.clone()).await {
                        warn!(
                            "failed to save OKX kline, inst_id:{}, timestamp:{}, error:{}",
                            kline.inst_id, kline.ts, error
                        );
                    }
                    broadcast_kline(&self.history_tx, kline).await;
                }
            }
        }
    }
}

pub struct OkxSyncHistoryService {
    okx_option_service: Arc<OptionService>,
    history_tx: mpsc::Sender<HistoryEntry>,
}

impl OkxSyncHistoryService {
    pub fn for_production(history_tx: mpsc::Sender<HistoryEntry>, okx_option_service: Arc<OptionService>) -> Result<Arc<Self>, YuError> {
        Ok(Arc::new(Self {
            okx_option_service,
            history_tx,
        }))
    }

    pub async fn broadcast_message(&self, kline: OkxKlinePo) {
        broadcast_kline(&self.history_tx, kline).await;
    }

    pub async fn check_and_broadcast_history(&self) -> Result<(), YuError> {
        for kline in self.okx_option_service.check_history_data().await? {
            self.broadcast_message(kline).await;
        }
        Ok(())
    }
}

async fn broadcast_kline(history_tx: &mpsc::Sender<HistoryEntry>, kline: OkxKlinePo) {
    let history_entry = history_entry_from_kline(&kline);
    if let Err(error) = history_tx.send(history_entry).await {
        error!(
            "failed to broadcast OKX kline, inst_id:{}, timestamp:{}, error:{}",
            kline.inst_id, kline.ts, error
        );
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

    ///
    /// 因为这里是通过websocket来同步的。
    ///
    async fn sync_all(&self) -> Result<(), YuError> {
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

pub struct OkxSyncOptionSummaryHistoryService {
    option_summary_repo: OkxOptionSummaryRepository,
    okx_option_service: Arc<OptionService>,
    history_tx: mpsc::Sender<HistoryEntry>,
}

impl OkxSyncOptionSummaryHistoryService {
    pub fn for_production(
        history_tx: mpsc::Sender<HistoryEntry>,
        okx_option_service: Arc<OptionService>,
        option_summary_repo: OkxOptionSummaryRepository,
    ) -> Arc<Self> {
        Arc::new(Self {
            option_summary_repo,
            okx_option_service,
            history_tx,
        })
    }

    pub async fn broadcast_message(&self, summary: OkxOptionSummaryPo) {
        let entry = history_entry_from_option_summary(&summary);
        if let Err(error) = self.history_tx.send(entry).await {
            error!(
                "failed to broadcast OKX option summary, inst_id:{}, timestamp:{}, error:{}",
                summary.inst_id, summary.server_ts, error
            );
        }
    }
}

#[async_trait]
impl SyncHistoryServiceTrait for OkxSyncOptionSummaryHistoryService {
    async fn query_and_broadcast_history(
        &self,
        inst_id: u64,
        start_ms: u64,
        end_ms: u64,
        batch_size: usize,
        tx: mpsc::Sender<Result<ServerMessage, Status>>,
    ) -> Result<(), YuError> {
        match self.option_summary_repo.find_summary_between(inst_id, start_ms, end_ms).await {
            Ok(summaries) => {
                if summaries.is_empty() {
                    let message = ServerMessage {
                        history: vec![],
                        server_timestamp: li::tools::time::unix_time_now_u64_utc(),
                    };
                    let _ = tx.send(Ok(message)).await;
                    return Ok(());
                }

                for batch in summaries.chunks(batch_size.max(1)) {
                    let message = ServerMessage {
                        history: batch.iter().map(history_entry_from_option_summary).collect(),
                        server_timestamp: li::tools::time::unix_time_now_u64_utc(),
                    };
                    if tx.send(Ok(message)).await.is_err() {
                        break;
                    }
                }
                Ok(())
            }
            Err(error) => {
                error!("query OKX option summary history failed: {error}");
                let status = Status::internal(format!("query OKX option summary history failed: {error}"));
                let _ = tx.send(Err(status.clone())).await;
                Err(status.into())
            }
        }
    }

    async fn check_history_data(&self) -> Result<(), YuError> {
        // OKX exposes current option-summary snapshots but no historical range endpoint to detect or repair gaps.
        log::debug!("skip OKX option summary history validation: historical snapshots are unavailable from the REST API");
        Ok(())
    }

    async fn initial_history_data(&self) -> Result<(), YuError> {
        // OKX exposes current option-summary snapshots but no historical range endpoint to detect or repair gaps.
        log::debug!("skip OKX option summary history Initial: historical snapshots are unavailable from the REST API");
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        for summary in self.okx_option_service.query_option_summary().await? {
            self.broadcast_message(summary).await;
        }
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::okx::duckdb_repository::{MockOkxKlineRepositoryTrait, OkxKlineRepository};
    use li::websocket::connection::MessageHandlerTrait;
    use tokio::sync::mpsc::error::TryRecvError;
    use yue::okx::models::websocket::{ArgBody, KlinePayload};

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

    #[test]
    fn converts_option_summary_to_sync_history_entry_preserving_optional_values() {
        let summary = OkxOptionSummaryPo {
            id: 11,
            inst_id: 7,
            inst_identify: "BTC-USD-260101-100000-C".to_string(),
            inst_type: "OPTION".to_string(),
            uly: Some("BTC-USD".to_string()),
            acquire_ts: 1_700_000_000_000,
            server_ts: 1_700_000_000_123,
            ask_vol: Some(0.25),
            bid_vol: None,
            delta: Some(0.5),
            delta_bs: None,
            fwd_px: Some(50_000.0),
            gamma: None,
            gamma_bs: Some(0.01),
            lever: None,
            mark_vol: Some(0.3),
            real_vol: None,
            vol_lv: Some(0.2),
            theta: None,
            theta_bs: Some(-0.1),
            vega: Some(0.05),
            vega_bs: None,
        };

        let entry = history_entry_from_option_summary(&summary);

        assert_eq!(HistoryType::try_from(entry.history_type), Ok(HistoryType::OkxOptionSummary));
        assert_eq!(InstrumentType::try_from(entry.instrument_type), Ok(InstrumentType::Okx));
        assert_eq!(entry.inst_id, summary.inst_id);
        let Some(history_entry::Payload::OkxOptionSummary(proto)) = entry.payload else {
            panic!("expected OKX option summary payload");
        };
        assert_eq!(proto.id, summary.id);
        assert_eq!(proto.inst_identify, summary.inst_identify);
        assert_eq!(proto.inst_type, summary.inst_type);
        assert_eq!(proto.uly, "BTC-USD");
        assert_eq!(proto.acquire_ts, summary.acquire_ts);
        assert_eq!(proto.server_ts, summary.server_ts);
        assert_eq!(proto.ask_vol, summary.ask_vol);
        assert_eq!(proto.bid_vol, summary.bid_vol);
        assert_eq!(proto.delta, summary.delta);
        assert_eq!(proto.delta_bs, summary.delta_bs);
        assert_eq!(proto.fwd_px, summary.fwd_px);
        assert_eq!(proto.gamma, summary.gamma);
        assert_eq!(proto.gamma_bs, summary.gamma_bs);
        assert_eq!(proto.lever, summary.lever);
        assert_eq!(proto.mark_vol, summary.mark_vol);
        assert_eq!(proto.real_vol, summary.real_vol);
        assert_eq!(proto.vol_lv, summary.vol_lv);
        assert_eq!(proto.theta, summary.theta);
        assert_eq!(proto.theta_bs, summary.theta_bs);
        assert_eq!(proto.vega, summary.vega);
        assert_eq!(proto.vega_bs, summary.vega_bs);
    }

    #[tokio::test]
    async fn websocket_handler_broadcasts_only_completed_live_klines() {
        let instrument = InstrumentPo::builder()
            .id(7)
            .inst_identify("BTC-USD-260101-100000-C".to_string())
            .inst_type("OPTION".to_string())
            .base_ccy("BTC".to_string())
            .list_time(1)
            .build();
        let live_instruments = Arc::new(RwLock::new(vec![instrument]));
        let (history_tx, mut history_rx) = mpsc::channel(2);
        let mut mock_kline_repo = MockOkxKlineRepositoryTrait::new();
        mock_kline_repo
            .expect_insert_history()
            .times(2)
            .withf(|kline| kline.inst_id == 7 && kline.confirm == 1)
            .returning(|_| Ok(()));
        let kline_repo: OkxKlineRepository = Arc::new(mock_kline_repo);
        let handler = OkxSyncKlineHandler::new(kline_repo, history_tx, live_instruments.clone());
        let payload = KlinePayload::builder()
            .arg(
                ArgBody::builder()
                    .channel("candle1H".to_string())
                    .inst_id("BTC-USD-260101-100000-C".to_string())
                    .build(),
            )
            .data(vec![
                vec![
                    "1700000000000".to_string(),
                    "1".to_string(),
                    "2".to_string(),
                    "0.5".to_string(),
                    "1.5".to_string(),
                    "3".to_string(),
                    "4".to_string(),
                    "5".to_string(),
                    "1".to_string(),
                ],
                vec!["0".to_string()],
            ])
            .build();

        handler.handle_message(&OkxWebsocketResponse::Kline(payload.clone())).await;

        let entry = history_rx.recv().await.expect("completed kline should be broadcast");
        assert_eq!(entry.inst_id, 7);
        assert!(matches!(entry.payload, Some(history_entry::Payload::OkxKline(_))));

        live_instruments.write().await.clear();
        handler.handle_message(&OkxWebsocketResponse::Kline(payload)).await;
        let cached_entry = history_rx.recv().await.expect("cached instrument should be broadcast");
        assert_eq!(cached_entry.inst_id, 7);
        assert!(matches!(history_rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn websocket_handler_ignores_non_live_instruments() {
        let live_instruments = Arc::new(RwLock::new(Vec::new()));
        let (history_tx, mut history_rx) = mpsc::channel(1);
        let kline_repo: OkxKlineRepository = Arc::new(MockOkxKlineRepositoryTrait::new());
        let handler = OkxSyncKlineHandler::new(kline_repo, history_tx, live_instruments);
        let payload = KlinePayload::builder()
            .arg(
                ArgBody::builder()
                    .channel("candle1H".to_string())
                    .inst_id("BTC-USD-260101-100000-C".to_string())
                    .build(),
            )
            .data(vec![vec![
                "1700000000000".to_string(),
                "1".to_string(),
                "2".to_string(),
                "0.5".to_string(),
                "1.5".to_string(),
                "3".to_string(),
                "4".to_string(),
                "5".to_string(),
                "1".to_string(),
            ]])
            .build();

        handler.handle_message(&OkxWebsocketResponse::Kline(payload)).await;

        assert!(matches!(history_rx.try_recv(), Err(TryRecvError::Empty)));
    }
}

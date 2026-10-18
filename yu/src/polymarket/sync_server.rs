use crate::errors::YuError;
use crate::polymarket::duckdb_repository::{PolyMarketInstrumentRepository, get_history_repo, get_instrument_repo};
use crate::polymarket::po::PolyMarketInstrumentPo;
use crate::polymarket::service::SeriesHistoryMarketService;
use crate::sync::models::grpc_sync::{
    ExchangeType, HistoryEntry, Instrument, InstrumentType, PolyMarketHistory, PolymarketInstrument, ServerMessage, history_entry,
};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::tools::time::{unix_time_now_u64_utc, unix_time_now_u64_utc_seconds};
use log::{error, info, trace};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use tonic::Status;
use yue::polymarket::restful_api::{PolymarketApi, default_polymarket_api};
use yue::tools::get_snow_flake_id_u64;

/**
把series id下所有的market分成open和close的。
返回顺序是open，和close
**/
async fn split_series_markets_with_client(
    api: PolymarketApi,
    series_id: String,
    existing_instruments: HashMap<String, u64>,
    inst_repo: PolyMarketInstrumentRepository,
) -> Result<(Vec<PolyMarketInstrumentPo>, Vec<PolyMarketInstrumentPo>), YuError> {
    let mut open_markets: Vec<PolyMarketInstrumentPo> = Vec::new();
    let mut close_markets: Vec<PolyMarketInstrumentPo> = Vec::new();
    let mut open_asset_markets_map: HashMap<String, PolyMarketInstrumentPo> = HashMap::new();
    let series = match api.query_series_by_id(&series_id, Some(false)).await {
        Ok(s) => s,
        Err(e) => {
            error!("query_series_by_id error: {:?}", e);
            return Err(YuError::from(e));
        }
    };
    let series_id_val = series.id;
    let series_slug = series.slug;
    let now = unix_time_now_u64_utc_seconds();
    if let Some(events) = series.events {
        trace!("split series_slug:{},events num:{}", series_slug, events.len());
        for event_in_series in events {
            let event_id = event_in_series.id;
            let event_slug = event_in_series.slug;
            match api.query_event_id(&event_id, None, None).await {
                Ok(event) => {
                    if let Some(markets) = event.markets {
                        trace!("event:{},market num:{}", event_slug, markets.len());
                        for market in markets {
                            // 看了一下，close的不一定存在start_date,所以以create_date为准。end_date不存在，则以now为准。
                            let create_date = market.created_at.unwrap_or(now + 1);
                            let start_data = market.start_date.unwrap_or(create_date);
                            let end_data = market.end_date.unwrap_or(now - 1);
                            let inst_slug = market.outcomes.unwrap();
                            for (idx, inst_id) in market.clob_token_ids.unwrap_or(vec![]).iter().enumerate() {
                                if open_asset_markets_map.contains_key(inst_id) {
                                    let open_market_prev = open_asset_markets_map.get(inst_id).unwrap();
                                    error!(
                                        "duplicate asset id found,asset_id:{},idx:{},,prev series_slug:{},prev event_slug:{}, perv market slug is {} and series_slug:{},event_slug:{},market slug:{}",
                                        inst_id,
                                        idx,
                                        open_market_prev.series_slug,
                                        open_market_prev.event_slug,
                                        open_market_prev.market_slug,
                                        series_slug,
                                        event_slug,
                                        market.slug
                                    );
                                    continue;
                                }
                                let mut po = PolyMarketInstrumentPo {
                                    id: get_snow_flake_id_u64(),
                                    series_id: series_id_val.clone(),
                                    series_slug: series_slug.clone(),
                                    event_id: event_id.clone(),
                                    event_slug: event_slug.clone(),
                                    market_id: market.id.clone(),
                                    market_slug: market.slug.clone(),
                                    asset_id: inst_id.clone(),
                                    asset_slug: format!("{}_{}", market.slug, inst_slug[idx]),
                                    start_ms: start_data * 1000,
                                    end_ms: end_data * 1000,
                                };
                                let id_db = existing_instruments.get(inst_id);
                                match id_db {
                                    Some(id) => {
                                        po.id = *id;
                                    }
                                    None => {
                                        inst_repo.insert_instrument(&po).await?;
                                    }
                                }
                                open_asset_markets_map.insert(inst_id.clone(), po.clone());
                                if now > start_data && now < end_data {
                                    open_markets.push(po)
                                } else {
                                    close_markets.push(po)
                                }
                            }
                        }
                    } else {
                        trace!("event:{},no market", event_slug);
                    }
                }
                Err(e) => {
                    error!("split series event id:{},error:{:?}", event_id, e);
                }
            }
        }
    } else {
        trace!("split series_slug:{},no events", series_slug);
    }
    info!(
        "{} has {} open market,{} close market",
        series_slug,
        open_markets.len(),
        close_markets.len()
    );
    Ok((open_markets, close_markets))
}

pub async fn get_all_instruments(
    series_ids: &Vec<String>,
    api: &PolymarketApi,
    inst_repo: PolyMarketInstrumentRepository,
) -> Result<(Vec<PolyMarketInstrumentPo>, Vec<PolyMarketInstrumentPo>), YuError> {
    let mut open_markets: Vec<PolyMarketInstrumentPo> = vec![];
    let mut close_markets: Vec<PolyMarketInstrumentPo> = vec![];

    // 并发为每个 series id 运行 split_series_markets
    let mut handles = Vec::with_capacity(series_ids.len());
    let existing_instruments = inst_repo.get_instrument_dictionary().await?;
    for id in series_ids {
        let id_cloned = id.clone();
        let client_cloned = api.clone();
        let inst_repo_cloned = inst_repo.clone();
        let existing_instrument_cloned = existing_instruments.clone();
        handles.push(tokio::spawn(async move {
            split_series_markets_with_client(client_cloned, id_cloned, existing_instrument_cloned, inst_repo_cloned).await
        }));
    }

    for handle in handles {
        match handle.await {
            Ok(Ok((mut open, mut close))) => {
                open_markets.append(&mut open);
                close_markets.append(&mut close);
            }
            Ok(Err(e)) => error!("split_series error: {:?}", e),
            Err(e) => error!("task join error: {:?}", e),
        }
    }
    Ok((open_markets, close_markets))
}

///
/// # 规则
/// 1. 订阅主要是以Series为单元，该series下面所有的event和market。
///
pub struct PolyMarketSyncInstrumentService {
    support_history_service: HashMap<InstrumentType, SyncHistoryService>,
    client: PolymarketApi,
    series_ids: Vec<String>,
    inst_repo: PolyMarketInstrumentRepository,
    live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    close_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
}

impl PolyMarketSyncInstrumentService {
    pub fn new(
        series_ids: Vec<String>,
        support_history_service: HashMap<InstrumentType, SyncHistoryService>,
        client: PolymarketApi,
        inst_repo: PolyMarketInstrumentRepository,
    ) -> SyncInstrumentService {
        Arc::new(Self {
            support_history_service,
            client,
            series_ids,
            inst_repo,
            live_instruments: Arc::new(RwLock::new(Vec::new())),
            close_instruments: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub fn for_production(series_ids: Vec<String>, support_history_service: HashMap<InstrumentType, SyncHistoryService>) -> Arc<Self> {
        let client = default_polymarket_api();
        let inst_repo = get_instrument_repo(None);
        Arc::new(Self {
            support_history_service,
            client,
            series_ids,
            inst_repo,
            live_instruments: Arc::new(RwLock::new(Vec::new())),
            close_instruments: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub async fn sync_instrument(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let (open, close) = get_all_instruments(&self.series_ids, &self.client, self.inst_repo.clone()).await?;
        {
            let mut instruments = self.live_instruments.write().await;
            *instruments = open.clone();
        }
        {
            let mut instruments = self.close_instruments.write().await;
            *instruments = close;
        }
        Ok(open)
    }

    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let mut res = self.live_instruments.read().await.clone();
        res.extend(self.close_instruments.read().await.clone());
        Ok(res)
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for PolyMarketSyncInstrumentService {
    async fn list_instruments(&self) -> Result<HashMap<String, Instrument>, YuError> {
        let polymarket_instruments = self.list_instruments().await;
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
                    let msg = ServerMessage {
                        history: vec![],
                        server_timestamp: unix_time_now_u64_utc(),
                    };
                    let _ = tx.send(Ok(msg)).await;
                    return Ok(());
                }

                for chunk in history_vec.chunks(batch_size) {
                    let mut histories = Vec::with_capacity(chunk.len());
                    for h in chunk.iter() {
                        let rpc_entry = PolyMarketHistory {
                            inst_id: h.instrument_id,
                            timestamp: h.timestamp,
                            price: h.price,
                        };
                        histories.push(HistoryEntry {
                            instrument_type: InstrumentType::PolymarketToken.into(),
                            payload: Some(history_entry::Payload::PolymarketHistory(rpc_entry)),
                        });
                    }
                    let msg = ServerMessage {
                        history: histories,
                        server_timestamp: unix_time_now_u64_utc(),
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

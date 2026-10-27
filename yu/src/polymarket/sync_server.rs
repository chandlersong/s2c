use crate::data_integrity::check::{DuckDBBinarySearchDataImpl, binary_search_gap};
use crate::data_integrity::models::ValidationGap;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::polymarket::db_consts::PolyMarketTables;
use crate::polymarket::duckdb_repository::{PolyMarketHistoryRepository, PolyMarketInstrumentRepository, get_history_repo, get_instrument_repo};
use crate::polymarket::po::{PolyMarketHistoryPo, PolyMarketInstrumentPo};
use crate::sync::models::grpc_sync::{
    HistoryEntry, HistoryType, Instrument, InstrumentType, PolyMarketHistory, PolymarketInstrument, ServerMessage, history_entry,
};
use crate::sync::server::sync_server::{SyncHistoryService, SyncHistoryServiceTrait, SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use li::tools::time::{unix_2_readable, unix_time_now_u64_utc, unix_time_now_u64_utc_seconds};
use log::{Level, debug, error, info, trace};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc::Sender};
use tonic::Status;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::{PolymarketApi, default_polymarket_api};
use yue::polymarket::restful_models::GetPricesHistoryQuery;
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
    support_history_service: HashMap<HistoryType, SyncHistoryService>,
    client: PolymarketApi,
    series_ids: Vec<String>,
    inst_repo: PolyMarketInstrumentRepository,
    live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    close_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
}

impl PolyMarketSyncInstrumentService {
    pub fn new(
        series_ids: Vec<String>,
        support_history_service: HashMap<HistoryType, SyncHistoryService>,
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

    pub fn for_production(
        series_ids: Vec<String>,
        support_history_service: HashMap<HistoryType, SyncHistoryService>,
        live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    ) -> Arc<Self> {
        let client = default_polymarket_api();
        let inst_repo = get_instrument_repo(None);
        Arc::new(Self {
            support_history_service,
            client,
            series_ids,
            inst_repo,
            live_instruments,
            close_instruments: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub async fn update_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
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

    pub async fn list_all_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let mut res = self.live_instruments.read().await.clone();
        res.extend(self.close_instruments.read().await.clone());
        Ok(res)
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for PolyMarketSyncInstrumentService {
    async fn list_instruments(&self) -> Result<Vec<Instrument>, YuError> {
        let polymarket_instruments = self.list_all_instruments().await;
        let mut res: Vec<Instrument> = Vec::new();
        match polymarket_instruments {
            Ok(list) => {
                for inst in list {
                    let poly = PolymarketInstrument::from(inst);
                    let instrument = Instrument {
                        instrument_type: InstrumentType::Polymarket as i32,
                        payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Polymarket(poly)),
                    };
                    res.push(instrument);
                }
            }
            Err(e) => {
                error!("list_instruments error: {:?}", e);
                return Err(Status::internal(format!("list polymarket instruments error: {:?}", e)).into());
            }
        };
        Ok(res)
    }

    async fn sync_instrument(&self) -> Result<Vec<Instrument>, YuError> {
        let instruments = self.update_instruments().await?;
        Ok(instruments
            .into_iter()
            .map(|instrument| Instrument {
                instrument_type: InstrumentType::Polymarket as i32,
                payload: Some(crate::sync::models::grpc_sync::instrument::Payload::Polymarket(
                    PolymarketInstrument::from(instrument),
                )),
            })
            .collect())
    }

    fn error_log(&self) -> String {
        "error when list polymarket instruments".to_string()
    }

    fn support_history_list(&self) -> Result<HashMap<HistoryType, SyncHistoryService>, YuError> {
        Ok(self.support_history_service.clone())
    }
}

pub struct PolyMarketSyncHistoryService {
    client: PolymarketApi,
    history_repo: PolyMarketHistoryRepository,
    interval: HistoryInterval,
    live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    history_tx: Sender<HistoryEntry>,
}

impl PolyMarketSyncHistoryService {
    pub fn new(
        client: PolymarketApi,
        interval: HistoryInterval,
        history_repo: PolyMarketHistoryRepository,
        live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
        history_tx: Sender<HistoryEntry>,
    ) -> Self {
        Self {
            client,
            history_repo,
            interval,
            live_instruments,
            history_tx,
        }
    }

    pub fn for_production(live_instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>, history_tx: Sender<HistoryEntry>) -> Arc<Self> {
        let client = default_polymarket_api();
        let interval = HistoryInterval::OneHour;
        let history_repo = get_history_repo(None, None);
        Arc::new(Self::new(client, interval, history_repo, live_instruments, history_tx))
    }

    ///
    /// 通过history_tx，
    /// 1. 转换成polymarket的historyEntry
    /// 2. 通过history_tx来发送
    ///
    pub async fn broadcast_message(&self, entry: PolyMarketHistoryPo, asset_slug: &str, timestamp: u64) {
        let history_entry = HistoryEntry {
            history_type: HistoryType::PolymarketToken.into(),
            instrument_type: InstrumentType::Polymarket.into(),
            inst_id: entry.instrument_id,
            payload: Some(history_entry::Payload::PolymarketHistory(PolyMarketHistory {
                timestamp,
                price: entry.price,
            })),
        };
        if let Err(e) = self.history_tx.send(history_entry).await {
            error!(
                "failed to broadcast polymarket history, asset_slug:{},timestamp:{},error:{}",
                asset_slug, timestamp, e
            );
        }
    }

    pub async fn query_and_insert_history(&self, query_payload: GetPricesHistoryQuery, inst: &PolyMarketInstrumentPo) -> Vec<PolyMarketHistoryPo> {
        let asset_id = inst.asset_id.clone();

        let query_payload_log = query_payload.clone();
        let query_payload_error_log = query_payload.clone();
        let end_timestamp = query_payload.end_ts.unwrap_or(unix_time_now_u64_utc_seconds() + 10);
        let start_timestamp = query_payload.start_ts.unwrap_or(unix_time_now_u64_utc_seconds() - 10);
        let history = self.client.query_prices_history(query_payload.clone()).await;
        let mut pre_history_data: HashMap<u64, u64> = HashMap::new();
        let mut res = vec![];
        match history {
            Ok(history) => {
                for h in history.history {
                    // only process points not later than end_timestamp
                    if h.t > end_timestamp || h.t < start_timestamp {
                        continue;
                    }
                    let timestamp = self.interval.get_close_unix_ms(h.t * 1000);
                    if pre_history_data.contains_key(&timestamp) {
                        let prev_t = pre_history_data.get(&timestamp).unwrap();
                        trace!(
                            "duplicate timestamp found, inst_id is {},prev_t is {},current t is {},\
                            query info, start:{},end:{},interval:{}",
                            asset_id,
                            prev_t,
                            h.t,
                            query_payload_log.start_ts.unwrap(),
                            query_payload_log.end_ts.unwrap(),
                            query_payload_log.interval.clone().unwrap().as_ref()
                        );
                        continue;
                    }
                    pre_history_data.insert(timestamp, h.t);
                    let entry = PolyMarketHistoryPo {
                        instrument_id: inst.id.clone(),
                        timestamp,
                        price: h.p,
                    };
                    res.push(entry.clone());
                    self.history_repo.insert_history(entry.clone()).await.unwrap_or_else(|e| {
                        error!(
                            "insert history error,asset_id:{},timestamp:{},price:{},error:{:?}",
                            asset_id, entry.timestamp, entry.price, e
                        );
                    });
                }
            }
            Err(e) => {
                error!("market:{},query prices history error: {:?}", inst.market_slug, e);
                error!("query param is: {}", query_payload_error_log)
            }
        }
        res
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
        match self.history_repo.get_history_between(inst_id, start_ms, end_ms).await {
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
                            timestamp: h.timestamp,
                            price: h.price,
                        };
                        histories.push(HistoryEntry {
                            history_type: HistoryType::PolymarketToken.into(),
                            instrument_type: InstrumentType::Polymarket.into(),
                            inst_id,
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

    ///
    /// FUTURE：
    /// 1. polymarket的数据本来就不完整。所以造成检查的时候，会反复拉取
    ///
    async fn check_history_data(&self) -> Result<(), YuError> {
        let binary_search_ds = DuckDBBinarySearchDataImpl::new(
            self.history_repo.get_db_provider(),
            PolyMarketTables::PriceHistory.table_name(),
            "timestamp".to_string(),
            "instrument_id".to_string(),
        );
        let now_ms = self.interval.get_now_close_unix_ms_utc();
        let interval_ms = self.interval.to_milliseconds();
        for instrument in self.live_instruments.read().await.iter() {
            //因为第一个时间点，应该是没有的。所以直接跳过。
            let inst_start = self.interval.get_close_unix_ms(instrument.start_ms) + interval_ms;
            let gaps: Vec<ValidationGap> = binary_search_gap(
                instrument.id.to_string().as_str(),
                "SPOT",
                inst_start,
                now_ms,
                &self.interval,
                binary_search_ds.clone(),
            )?;
            debug!("{} has {} gaps", instrument.asset_slug, gaps.len());
            let fidelity = self.interval.to_second() / 60;
            for g in gaps.iter() {
                match g {
                    ValidationGap::MissingData {
                        symbol,
                        trade_type,
                        start_time,
                        end_time,
                        table,
                    } => {
                        if log::log_enabled!(Level::Debug) {
                            //  FUTURE: 写入到一个csv之类的东西方便跟踪
                            debug!(
                                "found gap for instrument_id:{},start_ts:{},end_ts:{},symbol:{},trade_type:{},table:{}",
                                instrument.asset_slug,
                                unix_2_readable(start_time),
                                unix_2_readable(end_time),
                                symbol,
                                trade_type,
                                table
                            );
                        }

                        let start_ts = start_time / 1000;
                        let end_ts = end_time / 1000;
                        let query_param = GetPricesHistoryQuery {
                            market: instrument.asset_id.to_string(),
                            start_ts: Some(start_ts),
                            end_ts: Some(end_ts - 1),
                            interval: Some(self.interval.clone()),
                            fidelity: Some(fidelity.clone() as u32),
                        };
                        // index 可用于调试或区分不同 asset_id
                        let pos = self.query_and_insert_history(query_param, instrument).await;
                        for h in pos.iter() {
                            self.broadcast_message(h.clone(), &instrument.asset_slug, h.timestamp).await;
                        }
                    }
                    _ => {
                        error!("no gap found")
                    }
                }
            }
        }
        Ok(())
    }

    ///
    /// 更新规则。
    ///
    /// 开始时间：
    /// 1. 如果polymarket_price_history里面有数据，就从最大值到现在
    /// 2. 如果没有，就取polymarket_instruments中的start_ms
    ///
    async fn initial_history_data(&self, end_ms: Option<u64>) -> Result<(), YuError> {
        let end = end_ms.unwrap_or_else(|| self.interval.get_now_close_unix_ms_utc());
        let now = self.interval.get_close_unix_sec(end);
        let fidelity = self.interval.to_second() / 60;
        info!("start to initial polymarket history data");
        let max_timestamp_dictionary = self.history_repo.max_timestamp_group_by_inst_id().await?;
        let gap = self.interval.to_second() - 31;
        for instrument in self.live_instruments.read().await.iter() {
            //如果数据从polymarket_price_history来，那么最好+30s。这样防止重复，如果从polymarket_instruments，则往后
            let start_ts = match max_timestamp_dictionary.get(&instrument.id) {
                None => {
                    let res = self.interval.get_close_unix_ms(instrument.start_ms) + self.interval.to_milliseconds();
                    res.saturating_div(1000).saturating_sub(1)
                }
                Some(v) => v.saturating_div(1000).saturating_add(120),
            };
            if (start_ts >= now) || ((now - start_ts) < gap) {
                continue;
            }

            let query_param = GetPricesHistoryQuery {
                market: instrument.asset_id.clone(),
                start_ts: Some(start_ts),
                end_ts: Some(now.clone() + 120),
                interval: Some(self.interval.clone()),
                fidelity: Some(fidelity.clone() as u32),
            };
            // index 可用于调试或区分不同 asset_id
            self.query_and_insert_history(query_param, instrument).await;
        }
        info!("finish to initial polymarket history data");
        Ok(())
    }

    async fn sync_all(&self) -> Result<(), YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        //FUTURE: 这里加的时间不支持变动的interval。以让他offset的时间，能够支持动态的interval。
        let start_ts = now - self.interval.to_second() + 120; // 获取过去一小时的数据
        let end_ts = now + 60; // 获取过去一小时的数据
        let fidelity = self.interval.to_second() / 60;
        for instrument in self.live_instruments.read().await.iter() {
            let query_param = GetPricesHistoryQuery {
                market: instrument.asset_id.to_string(),
                start_ts: Some(start_ts),
                end_ts: Some(end_ts),
                interval: Some(self.interval.clone()),
                fidelity: Some(fidelity.clone() as u32),
            };
            // index 可用于调试或区分不同 asset_id
            let pos = self.query_and_insert_history(query_param, instrument).await;
            for h in pos.iter() {
                self.broadcast_message(h.clone(), &instrument.asset_slug, h.timestamp).await;
            }
        }
        Ok(())
    }

    async fn sync_period(&self) -> Result<Option<String>, YuError> {
        todo!()
    }
}

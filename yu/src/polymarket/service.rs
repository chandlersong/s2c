use crate::errors::YuError;
use crate::polymarket::duckdb_repository::{PolyMarketHistoryRepository, PolyMarketInstrumentRepository, get_history_repo, get_instrument_repo};
use crate::polymarket::po::{PolyMarketHistoryPo, PolyMarketInstrumentPo};
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc_seconds;
use log::{error, info, trace};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
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
                            // 如果不存在，按照polymarket的尿性,大概率是脏数据了。
                            let start_data = market.start_date.unwrap_or(now + 1);
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

async fn get_all_instruments(
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

#[cfg_attr(feature = "mockable", mockall::automock)]
#[async_trait]
pub trait SeriesHistoryMarketServiceTrait: Send + Sync {
    ///
    /// 获取正在监听的market。
    ///
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn sync_instrument(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn initial_history_data(&self) -> Result<(), YuError>;

    async fn query_instrument_history(&self, inst_id: u64, start_ts: u64) -> Result<Vec<PolyMarketHistoryPo>, YuError>;

    async fn fetch_latest_history(&self) -> Result<Vec<PolyMarketHistoryPo>, YuError>;

    fn subscribe_history_broadcast(&self) -> broadcast::Receiver<PolyMarketHistoryPo>;
}

pub type SeriesHistoryMarketService = Arc<dyn SeriesHistoryMarketServiceTrait>;

pub async fn default_series_history_market_service(series_ids: Vec<String>, interval: HistoryInterval) -> SeriesHistoryMarketService {
    let client = default_polymarket_api();
    let inst_repo = get_instrument_repo(None);
    let history_repo = get_history_repo(None, None);
    Arc::new(SeriesHistoryMarketServiceImpl::new(series_ids, interval, client, inst_repo, history_repo))
}

///
/// 这里主要是的作用，去获取相应的series下的所有market，然后获取这些market的历史数据。把获取的数据发送给下流处理。
///
/// 相应的主要内容为
/// 1.market的信息，以本地保存为主。也就是说维护的PolyMarketInstrumentPo，而不去维护其他信息。
/// 2.对外功能上来说，无非以下这些功能。
///     - 同步和更新本地额的instrument信息。
///     - 查询instrument的历史数据.
///     - 查询instrument的最新数据,然后分发。
///
pub struct SeriesHistoryMarketServiceImpl {
    series_ids: Vec<String>,
    interval: HistoryInterval,
    client: PolymarketApi,
    instruments: Arc<RwLock<Vec<PolyMarketInstrumentPo>>>,
    inst_repo: PolyMarketInstrumentRepository,
    history_repo: PolyMarketHistoryRepository,
    history_broadcast: broadcast::Sender<PolyMarketHistoryPo>,
}

impl SeriesHistoryMarketServiceImpl {
    fn new(
        series_ids: Vec<String>,
        interval: HistoryInterval,
        client: PolymarketApi,
        inst_repo: PolyMarketInstrumentRepository,
        history_repo: PolyMarketHistoryRepository,
    ) -> Self {
        let (history_broadcast, _) = broadcast::channel(1000);
        Self {
            series_ids,
            interval,
            client,
            instruments: Arc::new(RwLock::new(Vec::new())),
            inst_repo,
            history_repo,
            history_broadcast,
        }
    }

    pub async fn query_and_broadcast_history(&self, query_payload: GetPricesHistoryQuery, inst: &PolyMarketInstrumentPo) -> Vec<PolyMarketHistoryPo> {
        let asset_id = inst.asset_id.clone();

        let query_payload_log = query_payload.clone();
        let end_timestamp = query_payload.end_ts.unwrap_or(unix_time_now_u64_utc_seconds() + 10);
        let start_timestamp = query_payload.start_ts.unwrap_or(unix_time_now_u64_utc_seconds() - 10);
        let history = self.client.query_prices_history(query_payload).await;
        let mut pre_history_data: HashMap<u64, u64> = HashMap::new();
        let mut res = vec![];
        match history {
            Ok(history) => {
                for h in history.history {
                    // only process points not later than end_timestamp
                    if h.t > end_timestamp || h.t < start_timestamp {
                        continue;
                    }
                    let timestamp = self.interval.get_close_unix_ms(h.t);
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
                            query_payload_log.interval.clone().unwrap().to_string()
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
                    self.broadcast_message(entry, &inst.asset_slug, h.t);
                }
            }
            Err(e) => {
                error!("market:{},query prices history error: {:?}", inst.market_slug, e);
            }
        }
        res
    }

    fn broadcast_message(&self, entry: PolyMarketHistoryPo, asset_slug: &str, timestamp: u64) {
        if self.history_broadcast.receiver_count() != 0 {
            if let Err(e) = self.history_broadcast.send(entry) {
                error!("asset_slug:{}, timestamp {},history_broadcast error: {:?}", asset_slug, timestamp, e);
            }
        }
    }
}

/// 服务接口：抽象出 trait 方便在测试或其·它模块中 mock

#[async_trait::async_trait]
impl SeriesHistoryMarketServiceTrait for SeriesHistoryMarketServiceImpl {
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        Ok(self.instruments.read().await.clone())
    }

    async fn sync_instrument(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError> {
        let (open, _) = get_all_instruments(&self.series_ids, &self.client, self.inst_repo.clone()).await?;
        {
            let mut instruments = self.instruments.write().await;
            *instruments = open.clone();
        }
        Ok(open)
    }

    ///
    /// 更新规则。
    ///
    /// 开始时间：
    /// 1. 如果polymarket_price_history里面有数据，就从最大值到现在
    /// 2. 如果没有，就取polymarket_instruments中的start_ms
    ///
    async fn initial_history_data(&self) -> Result<(), YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        let fidelity = self.interval.to_second() / 60;
        info!("start to initial polymarket history data");
        let max_timestamp_dictionary = self.history_repo.get_max_timestamp_dictionary().await?;
        let gap = self.interval.to_second() - 31;
        for instrument in self.instruments.read().await.iter() {
            //如果数据从polymarket_price_history来，那么最好+30s。这样防止重复，如果从polymarket_instruments，则往后
            let start_ts = match max_timestamp_dictionary.get(&instrument.id) {
                None => instrument.start_ms.saturating_div(1000).saturating_sub(1),
                Some(v) => v.saturating_div(1000).saturating_add(30),
            };
            if (start_ts >= now) || ((now - start_ts) < gap) {
                continue;
            }

            let query_param = GetPricesHistoryQuery {
                market: instrument.asset_id.clone(),
                start_ts: Some(start_ts),
                end_ts: Some(now.clone() + 120),
                interval: Some(self.interval.as_ref().to_string()),
                fidelity: Some(fidelity.clone() as u32),
            };
            // index 可用于调试或区分不同 asset_id
            self.query_and_broadcast_history(query_param, instrument).await;
        }
        info!("finish to initial polymarket history data");
        Ok(())
    }

    async fn query_instrument_history(&self, inst_id: u64, start_ts: u64) -> Result<Vec<PolyMarketHistoryPo>, YuError> {
        self.history_repo.get_history_before(inst_id, start_ts).await
    }

    async fn fetch_latest_history(&self) -> Result<Vec<PolyMarketHistoryPo>, YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        let start_ts = now - self.interval.to_second() + 6 + 60; // 获取过去一小时的数据
        let end_ts = now + 60; // 获取过去一小时的数据
        let fidelity = self.interval.to_second() / 60;
        let mut res = vec![];
        for instrument in self.instruments.read().await.iter() {
            let query_param = GetPricesHistoryQuery {
                market: instrument.asset_id.to_string(),
                start_ts: Some(start_ts),
                end_ts: Some(end_ts),
                interval: Some(self.interval.as_ref().to_string()),
                fidelity: Some(fidelity.clone() as u32),
            };
            // index 可用于调试或区分不同 asset_id
            res.extend(self.query_and_broadcast_history(query_param, instrument).await);
        }
        Ok(res)
    }

    fn subscribe_history_broadcast(&self) -> broadcast::Receiver<PolyMarketHistoryPo> {
        self.history_broadcast.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use crate::errors::YuError;
    use crate::polymarket::duckdb_repository::{MockPolyMarketHistoryRepositoryTrait, MockPolyMarketInstrumentRepositoryTrait};
    use crate::polymarket::po::PolyMarketInstrumentPo;
    use crate::polymarket::service::{SeriesHistoryMarketServiceImpl, SeriesHistoryMarketServiceTrait};
    use serde_json::from_value;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use yue::models::HistoryInterval;
    use yue::polymarket::restful_api::{MockPolymarketApiTrait, PolymarketApi};
    use yue::polymarket::restful_models::{Event, GetPricesHistoryQuery, GetPricesHistoryResponse, MarketPriceHistoryPoint, Series};

    ///
    /// 测试目的主要是为了验证sync的逻辑是否正常。因为根据instrument的逻辑。
    /// 数据准备：
    /// 1. 返回的series和相应的market
    /// 2. 其中market m1的instrument已经存在于数据库中。但是market m2则是新的。
    ///
    /// 结果
    /// 1. 保存m2里面的两个token
    ///
    #[tokio::test]
    pub async fn test_sync_instrument() -> Result<(), YuError> {
        // 构造 Series / Event / Market JSON 并反序列化为结构体
        let series_json = json!({
            "id": "s1",
            "slug": "series1",
            "events": [ { "id": "e1", "slug": "event1" } ]
        });
        let series: Series = from_value(series_json).expect("deserialize series");

        let market1 = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["token11", "token12"],
            "outcomes": ["Yes", "No"]
        });

        let market2 = json!({
            "id": "m2",
            "slug": "market2",
            "conditionId": "c2",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["token21", "token22"],
            "outcomes": ["Yes", "No"]
        });
        // 仅保留 market_json 以便嵌入 event_json；无需单独绑定 market 变量
        let event_json = json!({
            "id": "e1",
            "slug": "event1",
            "markets": [ market1.clone(),market2.clone() ]
        });
        let event: Event = from_value(event_json).expect("deserialize event");

        // 设置 MockPolymarketClient
        let mut mock_api = MockPolymarketApiTrait::new();

        // query_series_by_id -> return series with minimal info
        let series_clone = series.clone();
        mock_api.expect_query_series_by_id().returning(move |_id, _| {
            let s = series_clone.clone();
            Ok(s)
        });

        // query_event_id -> return event with markets
        let event_clone = event.clone();
        mock_api.expect_query_event_id().returning(move |_id, _inc_chat, _inc_tmplt| {
            let e = event_clone.clone();
            Ok(e)
        });

        let mut mock_inst_repo = MockPolyMarketInstrumentRepositoryTrait::new();
        mock_inst_repo
            .expect_get_instrument_dictionary()
            .returning(|| Ok(HashMap::from([("token11".to_string(), 1), ("token12".to_string(), 2)])));

        mock_inst_repo
            .expect_insert_instrument()
            .withf(|po| po.asset_id == "token22")
            .times(1)
            .returning(|_| Ok(()));
        mock_inst_repo
            .expect_insert_instrument()
            .withf(|po| po.asset_id == "token21")
            .times(1)
            .returning(|_| Ok(()));

        let client: PolymarketApi = Arc::new(mock_api);
        let inst_repo = Arc::new(mock_inst_repo);
        let history_repo = Arc::new(MockPolyMarketHistoryRepositoryTrait::new());
        let series_ids = vec!["s1".to_string()];
        let service = SeriesHistoryMarketServiceImpl::new(series_ids, HistoryInterval::OneHour, client, inst_repo, history_repo);
        let instruments = service.sync_instrument().await?;
        assert_eq!(instruments.len(), 4);
        for instruments in instruments.iter() {
            if instruments.asset_id == "token11" {
                assert_eq!(instruments.id, 1);
            } else if instruments.asset_id == "token12" {
                assert_eq!(instruments.id, 2);
            }
        }

        Ok(())
    }

    ///
    /// 测试query_and_broadcast的两个功能。
    /// 1. 过滤返回超过结束时间的history
    /// 2. 会把history的timestamp规整。
    ///
    #[tokio::test]
    pub async fn test_query_and_broadcast() -> Result<(), YuError> {
        // 构造 Series / Event / Market JSON 并反序列化为结构体
        let interval = HistoryInterval::OneHour;
        // 仅保留 market_json 以便嵌入 event_json；无需单独绑定 market 变量
        // 因为真实情况，他会返回时间的东西。所以这里也就做过滤。
        let history_resp = GetPricesHistoryResponse {
            history: vec![
                MarketPriceHistoryPoint {
                    t: interval.get_now_close_unix_ms_utc() + 10,
                    p: 0.1,
                },
                MarketPriceHistoryPoint {
                    t: interval.get_now_close_unix_ms_utc() + 60 * 10,
                    p: 0.2,
                },
            ],
        };

        // 设置 MockPolymarketClient
        let mut mock_api = MockPolymarketApiTrait::new();

        // query_prices_history -> return history_resp
        let history_clone = history_resp.clone();
        mock_api.expect_query_prices_history().returning(move |_q| {
            let r = history_clone.clone();
            Ok(r)
        });

        let client: PolymarketApi = Arc::new(mock_api);
        let series_ids = vec!["s1".to_string()];
        let mock_inst_repo = MockPolyMarketInstrumentRepositoryTrait::new();
        let inst_repo = Arc::new(mock_inst_repo);
        let mut mock_history_repo = MockPolyMarketHistoryRepositoryTrait::new();
        let expected_timestamp = interval.get_now_close_unix_ms_utc();
        mock_history_repo
            .expect_insert_history()
            .times(1)
            .withf(move |po| po.price == 0.1 && po.timestamp == expected_timestamp)
            .returning(|_| Ok(()));
        let history_repo = Arc::new(mock_history_repo);

        let service = SeriesHistoryMarketServiceImpl::new(series_ids, HistoryInterval::OneHour, client, inst_repo, history_repo);
        let query_payload = GetPricesHistoryQuery {
            market: "market_slug".to_string(),
            start_ts: Some(interval.get_now_close_unix_ms_utc() + 10),
            end_ts: Some(interval.get_now_close_unix_ms_utc() + 60 * 10),
            interval: Some(interval.as_ref().to_string()),
            fidelity: Some((interval.to_second() / 60) as u32),
        };
        let inst = PolyMarketInstrumentPo::builder()
            .id(123)
            .series_id("series_id".to_string())
            .series_slug("series_slug".to_string())
            .event_id("event_id".to_string())
            .event_slug("event_slug".to_string())
            .market_slug("market_slug".to_string())
            .market_id("market_id".to_string())
            .asset_id("asset_id".to_string())
            .asset_slug("asset_slug".to_string())
            .start_ms(1)
            .end_ms(1)
            .build();
        let po = service.query_and_broadcast_history(query_payload, &inst).await;
        assert_eq!(po.len() == 1, true);

        Ok(())
    }
}

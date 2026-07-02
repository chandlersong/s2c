use crate::errors::YuError;
use crate::sync::sync_server::grpc_sync::PolyMarketHistory;
use async_trait::async_trait;
use li::tools::time::unix_time_now_u64_utc_seconds;
use log::{error, info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::PolymarketAPI;
use yue::polymarket::restful_models::{GetPricesHistoryQuery, Market};
pub struct MarketWithAddition {
    market: Market,
    series_id: String,
    series_slug: String,
    event_id: String,
    event_slug: String,
}
type MarketList = Arc<RwLock<Vec<MarketWithAddition>>>;

/**
把series id下所有的market分成open和close的。
返回顺序是open，和close
**/
async fn split_series_markets_with_client(
    api: PolymarketAPI,
    series_id: String,
) -> Result<(Vec<MarketWithAddition>, Vec<MarketWithAddition>), YuError> {
    let mut open_markets: Vec<MarketWithAddition> = Vec::new();
    let mut close_markets: Vec<MarketWithAddition> = Vec::new();
    let series = match api.query_series_by_id(&series_id, Some(false)).await {
        Ok(s) => s,
        Err(e) => {
            error!("query_series_by_id error: {:?}", e);
            return Err(crate::errors::YuError::from(e));
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
                            let market_with_addition = MarketWithAddition {
                                market: market.clone(),
                                series_id: series_id_val.clone(),
                                series_slug: series_slug.clone(),
                                event_id: event_id.clone(),
                                event_slug: event_slug.clone(),
                            };

                            if now > start_data && now < end_data {
                                open_markets.push(market_with_addition)
                            } else {
                                close_markets.push(market_with_addition)
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

async fn batch_split_series_markets_with_client(
    series_ids: &Vec<String>,
    api: PolymarketAPI,
) -> Result<(Vec<MarketWithAddition>, Vec<MarketWithAddition>), YuError> {
    let mut open_markets: Vec<MarketWithAddition> = vec![];
    let mut close_markets: Vec<MarketWithAddition> = vec![];

    // 并发为每个 series id 运行 split_series_markets
    let mut handles = Vec::with_capacity(series_ids.len());
    for id in series_ids {
        let id_cloned = id.clone();
        let client_cloned = api.clone();
        handles.push(tokio::spawn(
            async move { split_series_markets_with_client(client_cloned, id_cloned).await },
        ));
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
    async fn refresh_open_markets(&self) -> Result<(), YuError>;

    async fn initial_data(&self, start_timestamps: HashMap<String, u64>) -> Result<(), YuError>;

    async fn query_and_broadcast(&self, query_payload: GetPricesHistoryQuery, asset_index: usize, market: &MarketWithAddition);

    async fn fetch_last_one_hour_data(&self) -> Result<(), YuError>;
}

pub type SeriesHistoryMarketService = Arc<dyn SeriesHistoryMarketServiceTrait>;

pub async fn new_series_history_market_service(
    series_ids: Vec<String>,
    interval: HistoryInterval,
    history_broadcast: broadcast::Sender<PolyMarketHistory>,
    client: PolymarketAPI,
) -> SeriesHistoryMarketService {
    Arc::new(SeriesHistoryMarketServiceImpl::new(series_ids, interval, history_broadcast, client).await)
}

/**
1. series_ids下的close market的历史数据Kline数据
2. series_ids下，定时刷新还是运行的market的价格数据，
3. K线的周期，为interval

# polymarket的规则
1. 时间都是到秒。而不是毫秒
2. History的时间戳。一般也不会整点。会慢歌几秒
**/
pub struct SeriesHistoryMarketServiceImpl {
    series_ids: Vec<String>,
    interval: HistoryInterval,
    open_markets: MarketList,
    history_broadcast: broadcast::Sender<PolyMarketHistory>,
    client: PolymarketAPI,
}

impl SeriesHistoryMarketServiceImpl {
    pub async fn new(
        series_ids: Vec<String>,
        interval: HistoryInterval,
        history_broadcast: broadcast::Sender<PolyMarketHistory>,
        client: PolymarketAPI,
    ) -> Self {
        let (open_markets, _) = match batch_split_series_markets_with_client(&series_ids, client.clone()).await {
            Ok((open_markets, close_markets)) => (open_markets, close_markets),
            Err(e) => panic!("batch_split_series_markets error: {:?}", e),
        };
        info!(
            "SeriesHistoryMarketService: series num:{} , open markets num: {}",
            series_ids.len(),
            open_markets.len()
        );
        Self {
            series_ids,
            interval,
            open_markets: Arc::new(RwLock::new(open_markets)),
            history_broadcast,
            client,
        }
    }
}

impl SeriesHistoryMarketServiceImpl {
    fn broadcast_message(&self, entry: PolyMarketHistory, asset_slug: &str, timestamp: u64) -> broadcast::Sender<PolyMarketHistory> {
        if self.history_broadcast.receiver_count() != 0 {
            if let Err(e) = self.history_broadcast.send(entry) {
                error!("asset_slug:{}, timestamp {},history_broadcast error: {:?}", asset_slug, timestamp, e);
            }
        }
        self.history_broadcast.clone()
    }
}

/// 服务接口：抽象出 trait 方便在测试或其它模块中 mock

#[async_trait]
impl SeriesHistoryMarketServiceTrait for SeriesHistoryMarketServiceImpl {
    async fn refresh_open_markets(&self) -> Result<(), YuError> {
        match batch_split_series_markets_with_client(&self.series_ids, self.client.clone()).await {
            Ok((open_markets, _)) => {
                // 把 self.open_markets 替换成新的 open_markets
                let count = open_markets.len();
                let mut guard = self.open_markets.write().await;
                *guard = open_markets;
                info!("refresh_open_markets: updated open markets num: {}", count);
            }
            Err(e) => {
                error!("refresh_open_markets error: {:?}", e);
            }
        };

        Ok(())
    }

    async fn initial_data(&self, start_timestamps: HashMap<String, u64>) -> Result<(), YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        let fidelity = self.interval.to_second() / 60;
        info!("start to initial polymarket history data");
        for market in self.open_markets.read().await.iter() {
            match &market.market.clob_token_ids {
                None => {
                    warn!("market :{} has no clob_token_ids,skip", market.market.slug);
                }
                Some(asset_ids) => {
                    for (index, asset_id) in asset_ids.iter().enumerate() {
                        let start_ts = match start_timestamps.get(asset_id.as_str()) {
                            None => market.market.start_date.unwrap_or(0),
                            Some(v) => v.clone(),
                        };

                        let query_param = GetPricesHistoryQuery {
                            market: asset_id.to_string(),
                            start_ts: Some(start_ts - 1),
                            end_ts: Some(now.clone() + 10),
                            interval: Some(self.interval.as_ref().to_string()),
                            fidelity: Some(fidelity.clone() as u32),
                        };
                        // index 可用于调试或区分不同 asset_id
                        self.query_and_broadcast(query_param, index, &market).await;
                    }
                }
            }
        }
        info!("finish to initial polymarket history data");
        Ok(())
    }

    async fn query_and_broadcast(&self, query_payload: GetPricesHistoryQuery, asset_index: usize, market: &MarketWithAddition) {
        let asset_id = query_payload.market.clone();
        let asset_slug = match &market.market.outcomes {
            None => {
                format!("{}_{}", market.market.slug, asset_index)
            }
            Some(outcomes) => {
                format!("{}_{}", market.market.slug, outcomes[asset_index])
            }
        };
        let history = self.client.query_prices_history(query_payload).await;
        match history {
            Ok(history) => {
                for h in history.history {
                    let entry = PolyMarketHistory {
                        series_id: market.series_id.clone(),
                        series_slug: market.series_slug.clone(),
                        event_id: market.event_id.clone(),
                        event_slug: market.event_slug.clone(),
                        market_id: market.market.id.clone(),
                        market_slug: market.market.slug.clone(),
                        asset_id: asset_id.clone(),
                        asset_slug: asset_slug.clone(),
                        timestamp: h.t,
                        price: h.p,
                    };
                    self.broadcast_message(entry, &asset_slug, h.t);
                }
            }
            Err(e) => {
                error!("market:{},query prices history error: {:?}", market.market.slug, e);
            }
        }
    }

    async fn fetch_last_one_hour_data(&self) -> Result<(), YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        let fidelity = self.interval.to_second() / 60;
        for market in self.open_markets.read().await.iter() {
            match &market.market.clob_token_ids {
                None => {
                    warn!("market :{} has no clob_token_ids,skip", market.market.slug);
                }
                Some(asset_ids) => {
                    for (index, asset_id) in asset_ids.iter().enumerate() {
                        let start_ts = now - 3601; // 获取过去一小时的数据
                        let query_param = GetPricesHistoryQuery {
                            market: asset_id.to_string(),
                            start_ts: Some(start_ts - 1),
                            end_ts: Some(now.clone() + 10),
                            interval: Some(self.interval.as_ref().to_string()),
                            fidelity: Some(fidelity.clone() as u32),
                        };
                        // index 可用于调试或区分不同 asset_id
                        self.query_and_broadcast(query_param, index, &market).await;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::SeriesHistoryMarketServiceTrait;
    use crate::errors::YuError;
    use serde_json::from_value;
    use serde_json::json;
    use std::sync::Arc;
    use yue::models::HistoryInterval;
    use yue::polymarket::restful_api::{MockPolymarketApiTrait, PolymarketAPI};
    use yue::polymarket::restful_models::{Event, GetPricesHistoryQuery, GetPricesHistoryResponse, MarketPriceHistoryPoint, Series};

    #[tokio::test]
    pub async fn test_series_history_service_with_mock_client() -> Result<(), YuError> {
        // 构造 Series / Event / Market JSON 并反序列化为结构体
        let series_json = json!({
            "id": "s1",
            "slug": "series1",
            "events": [ { "id": "e1", "slug": "event1" } ]
        });
        let series: Series = from_value(series_json).expect("deserialize series");

        let market_json = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["tokenA"],
            "outcomes": ["Yes", "No"]
        });
        // 仅保留 market_json 以便嵌入 event_json；无需单独绑定 market 变量
        let event_json = json!({
            "id": "e1",
            "slug": "event1",
            "markets": [ market_json.clone() ]
        });
        let event: Event = from_value(event_json).expect("deserialize event");

        // prices history response
        let history_resp = GetPricesHistoryResponse {
            history: vec![MarketPriceHistoryPoint { t: 1000, p: 0.42 }],
        };

        // 设置 MockPolymarketClient
        let mut mock = MockPolymarketApiTrait::new();

        // query_series_by_id -> return series with minimal info
        let series_clone = series.clone();
        mock.expect_query_series_by_id().returning(move |_id, _| {
            let s = series_clone.clone();
            Ok(s)
        });

        // query_event_id -> return event with markets
        let event_clone = event.clone();
        mock.expect_query_event_id().returning(move |_id, _inc_chat, _inc_tmplt| {
            let e = event_clone.clone();
            Ok(e)
        });

        // query_prices_history -> return history_resp
        let history_clone = history_resp.clone();
        mock.expect_query_prices_history().returning(move |_q| {
            let r = history_clone.clone();
            Ok(r)
        });

        let client: PolymarketAPI = Arc::new(mock);

        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let series_ids = vec!["s1".to_string()];
        let interval = HistoryInterval::OneMinute;

        let svc = super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client).await;

        // 调用 fetch_last_one_hour_data，会使用 mock 返回的 history 并通过 broadcast 发送
        svc.fetch_last_one_hour_data().await?;

        // 接收一条消息
        let received = rx.recv().await.expect("should receive history");
        assert_eq!(received.price, 0.42_f64);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_initial_data_with_mock_client() -> Result<(), YuError> {
        // 构造 Series / Event / Market JSON 并反序列化为结构体
        let series_json = json!({
            "id": "s1",
            "slug": "series1",
            "events": [ { "id": "e1", "slug": "event1" } ]
        });
        let series: Series = from_value(series_json).expect("deserialize series");

        let market_json = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["tokenA"],
            "outcomes": ["Yes", "No"]
        });
        let event_json = json!({
            "id": "e1",
            "slug": "event1",
            "markets": [ market_json.clone() ]
        });
        let event: Event = from_value(event_json).expect("deserialize event");

        // prices history response
        let history_resp = GetPricesHistoryResponse {
            history: vec![MarketPriceHistoryPoint { t: 1000, p: 0.42 }],
        };

        // 设置 MockPolymarketClient
        let mut mock = MockPolymarketApiTrait::new();

        // query_series_by_id -> return series with minimal info
        let series_clone = series.clone();
        mock.expect_query_series_by_id().returning(move |_id, _| {
            let s = series_clone.clone();
            Ok(s)
        });

        // query_event_id -> return event with markets
        let event_clone = event.clone();
        mock.expect_query_event_id().returning(move |_id, _inc_chat, _inc_tmplt| {
            let e = event_clone.clone();
            Ok(e)
        });

        // query_prices_history -> return history_resp
        let history_clone = history_resp.clone();
        mock.expect_query_prices_history().returning(move |_q| {
            let r = history_clone.clone();
            Ok(r)
        });

        let client: PolymarketAPI = Arc::new(mock);

        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let series_ids = vec!["s1".to_string()];
        let interval = HistoryInterval::OneMinute;

        let svc = super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client).await;

        // 构造 start_timestamps，覆盖 tokenA 的起始时间
        let mut start_ts_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        start_ts_map.insert("tokenA".to_string(), 900u64);

        // 调用 initial_data，会使用 mock 返回的 history 并通过 broadcast 发送
        svc.initial_data(start_ts_map).await?;

        // 接收一条消息
        let received = rx.recv().await.expect("should receive history");
        assert_eq!(received.price, 0.42_f64);
        Ok(())
    }

    // 测试1：当 start_ts_map 包含 token 的时候，应该使用 start_ts_map 提供的起始时间
    #[tokio::test]
    pub async fn test_initial_data_uses_start_ts_map() -> Result<(), YuError> {
        let series_json = json!({
            "id": "s1",
            "slug": "series1",
            "events": [ { "id": "e1", "slug": "event1" } ]
        });
        let series: Series = from_value(series_json).expect("deserialize series");

        let market_json = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["tokenA"],
            "outcomes": ["Yes", "No"]
        });
        let event_json = json!({
            "id": "e1",
            "slug": "event1",
            "markets": [ market_json.clone() ]
        });
        let event: Event = from_value(event_json).expect("deserialize event");

        let history_resp = GetPricesHistoryResponse {
            history: vec![MarketPriceHistoryPoint { t: 1000, p: 0.42 }],
        };

        // 用 mock 检查传入的 query.start_ts 是否等于 start_ts_map - 1
        let mut mock = MockPolymarketApiTrait::new();
        let series_clone = series.clone();
        mock.expect_query_series_by_id().returning(move |_id, _| Ok(series_clone.clone()));
        let event_clone = event.clone();
        mock.expect_query_event_id()
            .returning(move |_id, _inc_chat, _inc_tmplt| Ok(event_clone.clone()));

        let history_clone = history_resp.clone();
        mock.expect_query_prices_history()
            .withf(move |q: &GetPricesHistoryQuery| q.market == "tokenA" && q.start_ts == Some(900u64 - 1))
            .returning(move |_q| Ok(history_clone.clone()));

        let client: PolymarketAPI = Arc::new(mock);
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let series_ids = vec!["s1".to_string()];
        let interval = HistoryInterval::OneMinute;
        let svc = super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client).await;

        let mut start_ts_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        start_ts_map.insert("tokenA".to_string(), 900u64);

        svc.initial_data(start_ts_map).await?;

        let received = rx.recv().await.expect("should receive history");
        assert_eq!(received.price, 0.42_f64);
        Ok(())
    }

    // 测试2：当 start_ts_map 不包含 token 时，应该使用 market.start_date（如果存在）作为起始时间
    #[tokio::test]
    pub async fn test_initial_data_uses_market_start_date_if_missing() -> Result<(), YuError> {
        let series_json = json!({
            "id": "s1",
            "slug": "series1",
            "events": [ { "id": "e1", "slug": "event1" } ]
        });
        let series: Series = from_value(series_json).expect("deserialize series");

        let market_json = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["tokenA"],
            "outcomes": ["Yes", "No"]
        });
        let event_json = json!({
            "id": "e1",
            "slug": "event1",
            "markets": [ market_json.clone() ]
        });
        let event: Event = from_value(event_json).expect("deserialize event");

        // 计算 market.start_date（由反序列化产生）并期望 query.start_ts == start_date - 1
        let market_start = event.markets.as_ref().unwrap()[0].start_date.unwrap();
        let expected_sent_start = market_start - 1;

        let history_resp = GetPricesHistoryResponse {
            history: vec![MarketPriceHistoryPoint { t: 1000, p: 0.99 }],
        };

        let mut mock = MockPolymarketApiTrait::new();
        let series_clone = series.clone();
        mock.expect_query_series_by_id().returning(move |_id, _| Ok(series_clone.clone()));
        let event_clone = event.clone();
        mock.expect_query_event_id()
            .returning(move |_id, _inc_chat, _inc_tmplt| Ok(event_clone.clone()));

        let history_clone = history_resp.clone();
        mock.expect_query_prices_history()
            .withf(move |q: &GetPricesHistoryQuery| q.market == "tokenA" && q.start_ts == Some(expected_sent_start))
            .returning(move |_q| Ok(history_clone.clone()));

        let client: PolymarketAPI = Arc::new(mock);
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let series_ids = vec!["s1".to_string()];
        let interval = HistoryInterval::OneMinute;
        let svc = super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client).await;

        // 传入空的 start_ts_map
        let start_ts_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        svc.initial_data(start_ts_map).await?;

        let received = rx.recv().await.expect("should receive history");
        assert_eq!(received.price, 0.99_f64);
        Ok(())
    }
}

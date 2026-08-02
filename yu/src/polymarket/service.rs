use crate::duck_db::{DuckDBDSProvider, DuckDBPO};
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::polymarket::db_consts::PolyMarketTables::AssertInfo;
use crate::polymarket::po::{PolyMarketHistoryPo, PolyMarketInstrumentPo};
use crate::sync::models::grpc_sync::PolyMarketHistory;
use async_trait::async_trait;
use li::tools::time::{UnixTimeStamp, unix_time_now_u64_utc_seconds};
use log::{error, info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::PolymarketApi;
use yue::polymarket::restful_models::{GetPricesHistoryQuery, Market};
use yue::tools::get_snow_flake_id_u64;

/**
把series id下所有的market分成open和close的。
返回顺序是open，和close
**/
async fn split_series_markets_with_client(
    api: PolymarketApi,
    series_id: String,
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
                                let market_with_addition = PolyMarketInstrumentPo {
                                    id: get_snow_flake_id_u64(),
                                    series_id: series_id_val.clone(),
                                    series_slug: series_slug.clone(),
                                    event_id: event_id.clone(),
                                    event_slug: event_slug.clone(),
                                    market_id: market.id.clone(),
                                    market_slug: market.slug.clone(),
                                    asset_id: inst_id.clone(),
                                    asset_slug: format!("{}_{}", market.slug, inst_slug[idx]),
                                    start_ms: start_data,
                                    end_ms: end_data,
                                };
                                open_asset_markets_map.insert(inst_id.clone(), market_with_addition.clone());
                                if now > start_data && now < end_data {
                                    open_markets.push(market_with_addition)
                                } else {
                                    close_markets.push(market_with_addition)
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
) -> Result<(Vec<PolyMarketInstrumentPo>, Vec<PolyMarketInstrumentPo>), YuError> {
    let mut open_markets: Vec<PolyMarketInstrumentPo> = vec![];
    let mut close_markets: Vec<PolyMarketInstrumentPo> = vec![];

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
    ///
    /// 获取正在监听的market。
    ///
    async fn list_instruments(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn sync_instrument(&self) -> Result<Vec<PolyMarketInstrumentPo>, YuError>;

    async fn initial_history_data(&self, retention_ms: Option<UnixTimeStamp>) -> Result<(), YuError>;

    async fn query_instrument_history(
        &self,
        query_payload: GetPricesHistoryQuery,
        instrument: &PolyMarketInstrumentPo,
    ) -> Result<Vec<PolyMarketHistoryPo>, YuError>;

    async fn fetch_latest_history(&self) -> Result<Vec<PolyMarketHistoryPo>, YuError>;
}

pub type SeriesHistoryMarketService = Arc<dyn SeriesHistoryMarketServiceTrait>;

pub async fn new_series_history_market_service(
    series_ids: Vec<String>,
    interval: HistoryInterval,
    client: PolymarketApi,
    ds_provider: Option<DuckDBDSProvider>,
) -> SeriesHistoryMarketService {
    Arc::new(SeriesHistoryMarketServiceImpl::new(series_ids, interval, client))
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
}

impl SeriesHistoryMarketServiceImpl {
    fn new(series_ids: Vec<String>, interval: HistoryInterval, client: PolymarketApi) -> Self {
        Self {
            series_ids,
            interval,
            client,
            instruments: Arc::new(RwLock::new(Vec::new())),
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
        let (open, _) = get_all_instruments(&self.series_ids, &self.client).await?;
        {
            let mut instruments = self.instruments.write().await;
            *instruments = open.clone();
        }
        Ok(open)
    }

    async fn initial_history_data(&self, retention_ms: Option<UnixTimeStamp>) -> Result<(), YuError> {
        todo!()
    }

    async fn query_instrument_history(
        &self,
        query_payload: GetPricesHistoryQuery,
        instrument: &PolyMarketInstrumentPo,
    ) -> Result<Vec<PolyMarketHistoryPo>, YuError> {
        todo!()
    }

    async fn fetch_latest_history(&self) -> Result<Vec<PolyMarketHistoryPo>, YuError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use crate::errors::YuError;
    use crate::test_utils::create_memory_duckdb_provider;
    use serde_json::from_value;
    use serde_json::json;
    use std::sync::Arc;
    use yue::models::HistoryInterval;
    use yue::polymarket::restful_api::{MockPolymarketApiTrait, PolymarketApi};
    use yue::polymarket::restful_models::{Event, GetPricesHistoryResponse, MarketPriceHistoryPoint, Series};

    #[tokio::test]
    pub async fn test_series_history_service_with_mock_client() -> Result<(), YuError> {
        // 构造 Series / Event / Market JSON 并反序列化为结构体
        let interval = HistoryInterval::OneMinute;
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
            history: vec![MarketPriceHistoryPoint {
                t: interval.get_now_close_unix_sec_utc(),
                p: 0.42,
            }],
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

        let client: PolymarketApi = Arc::new(mock);

        // let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let series_ids = vec!["s1".to_string()];

        let (provider, _) = create_memory_duckdb_provider();

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

        let client: PolymarketApi = Arc::new(mock);

        let series_ids = vec!["s1".to_string()];
        let interval = HistoryInterval::OneMinute;
        let (provider, _) = create_memory_duckdb_provider();

        Ok(())
    }
}

use crate::duck_db::{DuckDBDSProvider, DuckDBPO};
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::polymarket::db_consts::PolyMarketTables::AssertInfo;
use crate::polymarket::po::PolyMarketAssetInfoPo;
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
use yue::query_message::DataSourceProviderTrait;

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
    ds_provider: Option<DuckDBDSProvider>,
    assert_infos: Arc<RwLock<Vec<PolyMarketAssetInfoPo>>>,
) -> SeriesHistoryMarketService {
    Arc::new(SeriesHistoryMarketServiceImpl::new(series_ids, interval, history_broadcast, client, ds_provider, assert_infos).await)
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
    ds_provider: DuckDBDSProvider,
    assert_infos: Arc<RwLock<Vec<PolyMarketAssetInfoPo>>>,
}

impl SeriesHistoryMarketServiceImpl {
    pub async fn new(
        series_ids: Vec<String>,
        interval: HistoryInterval,
        history_broadcast: broadcast::Sender<PolyMarketHistory>,
        client: PolymarketAPI,
        ds_provider: Option<DuckDBDSProvider>,
        assert_infos: Arc<RwLock<Vec<PolyMarketAssetInfoPo>>>,
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
            ds_provider: ds_provider.unwrap_or_else(|| DuckDBDSProvider::default()),
            assert_infos,
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

    ///
    /// 检查过程。
    /// 1. 通过 select assert_id from assert_info来获取所有的asset_id
    /// 2. loop market。如果asset_id已经存在，则跳过。
    /// 3. 不存在，则组装一个PolyMarketAssertInfoPo，存入数据库
    ///
    /// 返回所有数据库中的PolyMarketAssertInfoPo
    ///
    async fn refresh_assert_info_in_db(&self, markets: &Vec<MarketWithAddition>) -> Result<Vec<PolyMarketAssetInfoPo>, YuError> {
        // acquire a connection
        let mut conn = match self.ds_provider.acquire() {
            Ok(c) => c,
            Err(e) => {
                error!("acquire connection error when refresh assert_info_in_db: {:?}", e);
                return Err(YuError::from(e));
            }
        };

        // 读取已存在的条目并同时收集 assert_id
        let mut existing: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut all_pos: Vec<PolyMarketAssetInfoPo> = Vec::new();
        let sql = "SELECT series_id, series_slug, event_id, event_slug, market_id, market_slug, assert_id, assert_slug FROM polymarket_assert_info;";
        match conn.prepare(sql) {
            Ok(mut stmt) => {
                match stmt.query([]) {
                    Ok(mut rows) => {
                        while let Some(row_res) = rows.next().map_err(|e| e.to_string()).ok() {
                            match row_res {
                                Some(row) => {
                                    let series_id: String = row.get(0).unwrap_or_default();
                                    let series_slug: String = row.get(1).unwrap_or_default();
                                    let event_id: String = row.get(2).unwrap_or_default();
                                    let event_slug: String = row.get(3).unwrap_or_default();
                                    let market_id: String = row.get(4).unwrap_or_default();
                                    let market_slug: String = row.get(5).unwrap_or_default();
                                    if let Ok(assert_id) = row.get::<usize, String>(6) {
                                        let assert_slug: String = row.get(7).unwrap_or_default();
                                        existing.insert(assert_id.clone());
                                        all_pos.push(PolyMarketAssetInfoPo {
                                            series_id: series_id.clone(),
                                            series_slug: series_slug.clone(),
                                            event_id: event_id.clone(),
                                            event_slug: event_slug.clone(),
                                            market_id: market_id.clone(),
                                            market_slug: market_slug.clone(),
                                            asset_id: assert_id,
                                            asset_slug: assert_slug,
                                        });
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                    Err(e) => {
                        // 如果查询失败，很可能是表不存在，尝试创建表后继续（但这里先记录错误并继续）
                        error!("query existing assert_info failed: {:?}, will attempt to create table", e);
                        if let Err(e2) = conn.execute_batch(crate::polymarket::db_consts::CREATE_POLYMARKET_ASSERT_INFO_TABLE) {
                            error!("failed to create poly_market_assert_info table: {:?}", e2);
                            return Err(YuError::from(e2));
                        }
                    }
                }
            }
            Err(e) => {
                // 如果 prepare 失败，很可能是因为表不存在，尝试创建表
                error!("prepare select assert_info failed: {:?}, will attempt to create table", e);
                if let Err(e2) = conn.execute_batch(crate::polymarket::db_consts::CREATE_POLYMARKET_ASSERT_INFO_TABLE) {
                    error!("failed to create poly_market_assert_info table: {:?}", e2);
                    return Err(crate::errors::YuError::from(e2));
                }
            }
        }

        let mut new_pos: Vec<PolyMarketAssetInfoPo> = Vec::new();

        for market in markets.iter() {
            match &market.market.clob_token_ids {
                None => {
                    warn!("market :{} has no clob_token_ids,skip", market.market.slug);
                }
                Some(asset_ids) => {
                    for (index, asset_id) in asset_ids.iter().enumerate() {
                        if existing.contains(asset_id) {
                            continue;
                        }

                        // 构造 asset_slug
                        let outcome = match &market.market.outcomes {
                            None => index.to_string(),
                            Some(outcomes) => outcomes.get(index).cloned().unwrap_or(index.to_string()),
                        };
                        let po = PolyMarketAssetInfoPo {
                            series_id: market.series_id.clone(),
                            series_slug: market.series_slug.clone(),
                            event_id: market.event_id.clone(),
                            event_slug: market.event_slug.clone(),
                            market_id: market.market.id.clone(),
                            market_slug: market.market.slug.clone(),
                            asset_id: asset_id.clone(),
                            asset_slug: format!("{}_{}", market.market.slug, outcome),
                        };

                        new_pos.push(po);
                    }
                }
            }
        }
        info!("find new markets:{}", new_pos.len());

        if !new_pos.is_empty() {
            // write batch using transaction + appender
            let mut tx = match conn.transaction() {
                Ok(t) => t,
                Err(e) => {
                    error!("create transaction failed when insert assert_info: {:?}", e);
                    return Err(YuError::from(e));
                }
            };
            tx.set_drop_behavior(duckdb::DropBehavior::Commit);
            let mut appender = match tx.appender(AssertInfo.table_name().as_str()) {
                Ok(a) => a,
                Err(e) => {
                    error!("Failed to create appender for poly_market_assert_info: {:?}", e);
                    return Err(YuError::from(e));
                }
            };

            // clone new_pos so we can extend all_pos later without moving
            let to_insert = new_pos.clone();
            for po in to_insert.iter() {
                if let Err(e) = appender.append_row(po.to_params()) {
                    error!("Failed to append assert_info row: {:?}", e);
                    return Err(YuError::from(e));
                }
            }

            if let Err(e) = appender.flush() {
                error!("Failed to flush appender for poly_market_assert_info: {:?}", e);
                return Err(YuError::from(e));
            }

            // 把新插入的条目合并到 all_pos，保持顺序：已有的在前，新插入的在后
            all_pos.extend(new_pos.into_iter());
        }

        Ok(all_pos)
    }
}

/// 服务接口：抽象出 trait 方便在测试或其它模块中 mock

#[async_trait]
impl SeriesHistoryMarketServiceTrait for SeriesHistoryMarketServiceImpl {
    async fn refresh_open_markets(&self) -> Result<(), YuError> {
        match batch_split_series_markets_with_client(&self.series_ids, self.client.clone()).await {
            Ok((open_markets, _)) => {
                // 把 self.open_markets 替换成新的 open_markets
                let asset_infos = self.refresh_assert_info_in_db(&open_markets).await?;
                {
                    let mut guard = self.assert_infos.write().await;
                    *guard = asset_infos;
                };
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
                        // tests expect query.start_ts to be start_ts - 1, so use saturating_sub to avoid underflow
                        let query_start = start_ts.saturating_sub(1);
                        if (query_start > now) || ((now - start_ts) < self.interval.to_second()) {
                            continue;
                        }

                        let query_param = GetPricesHistoryQuery {
                            market: asset_id.to_string(),
                            start_ts: Some(query_start),
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
        let end_timestamp = query_payload.end_ts.unwrap_or(unix_time_now_u64_utc_seconds() + 10);
        let history = self.client.query_prices_history(query_payload).await;

        match history {
            Ok(history) => {
                for h in history.history {
                    // only process points not later than end_timestamp
                    if h.t > end_timestamp {
                        continue;
                    }
                    let timestamp = self.interval.get_close_unix_sec(h.t);
                    let entry = PolyMarketHistory {
                        asset_id: asset_id.clone(),
                        timestamp,
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
                        let start_ts = now - 3599; // 获取过去一小时的数据
                        let query_param = GetPricesHistoryQuery {
                            market: asset_id.to_string(),
                            start_ts: Some(start_ts),
                            end_ts: Some(now.clone() + 30),
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
    use super::MarketWithAddition;
    use super::SeriesHistoryMarketServiceTrait;
    use crate::errors::YuError;
    use crate::test_utils::{create_memory_db_provider, create_memory_duckdb_provider};
    use serde_json::from_value;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use yue::models::HistoryInterval;
    use yue::polymarket::restful_api::{MockPolymarketApiTrait, PolymarketAPI};
    use yue::polymarket::restful_models::{Event, GetPricesHistoryQuery, GetPricesHistoryResponse, Market, MarketPriceHistoryPoint, Series};
    use yue::query_message::DataSourceProviderTrait;

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
        let (provider, _) = create_memory_duckdb_provider();
        let svc =
            super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client, Some(provider), Arc::new(RwLock::new(vec![]))).await;

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
        let (provider, _) = create_memory_duckdb_provider();
        let svc =
            super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client, Some(provider), Arc::new(RwLock::new(vec![]))).await;

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
        let (provider, _) = create_memory_duckdb_provider();
        let svc =
            super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client, Some(provider), Arc::new(RwLock::new(vec![]))).await;

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
        let (provider, _) = create_memory_duckdb_provider();
        let svc =
            super::SeriesHistoryMarketServiceImpl::new(series_ids, interval, tx.clone(), client, Some(provider), Arc::new(RwLock::new(vec![]))).await;

        // 传入空的 start_ts_map
        let start_ts_map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        svc.initial_data(start_ts_map).await?;

        let received = rx.recv().await.expect("should receive history");
        assert_eq!(received.price, 0.99_f64);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_refresh_assert_info_in_db_inserts_rows() -> Result<(), YuError> {
        // 构造 market
        let market_json = json!({
            "id": "m1",
            "slug": "market1",
            "conditionId": "c1",
            "marketMakerAddress": "addr",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2027-01-01T00:00:00Z",
            "clobTokenIds": ["tokenA", "tokenB"],
            "outcomes": ["Yes", "No"]
        });
        let market: Market = from_value(market_json).expect("deserialize market");
        let mware = MarketWithAddition {
            market: market.clone(),
            series_id: "s1".to_string(),
            series_slug: "series1".to_string(),
            event_id: "e1".to_string(),
            event_slug: "event1".to_string(),
        };
        let markets = vec![mware];

        let provider = create_memory_db_provider();
        // create both tables
        crate::polymarket::database::initial_tables(Some(provider.clone())).expect("init tables");

        // pre-insert tokenA so refresh should skip it
        let pre_conn = provider.acquire().expect("acquire");
        pre_conn.execute("INSERT INTO polymarket_assert_info(assert_id, series_id, series_slug, event_id, event_slug, market_id, market_slug, assert_slug) VALUES ('tokenA','s1','series1','e1','event1','m1','market1','market1_Yes')", []).expect("insert tokenA");

        let mock = MockPolymarketApiTrait::new();
        let client: PolymarketAPI = Arc::new(mock);
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        let series_ids: Vec<String> = vec![]; // keep empty so new() won't call remote
        let interval = HistoryInterval::OneMinute;
        let svc = super::SeriesHistoryMarketServiceImpl::new(
            series_ids,
            interval,
            tx.clone(),
            client,
            Some(provider.clone()),
            Arc::new(RwLock::new(vec![])),
        )
        .await;

        // 调用私有方法并获取数据库中所有记录
        let all = svc.refresh_assert_info_in_db(&markets).await.expect("refresh assert info");
        // should have tokenA (preexisting) and tokenB (new)
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|p| p.asset_id == "tokenA" && p.asset_slug == "market1_Yes"));
        assert!(all.iter().any(|p| p.asset_id == "tokenB" && p.asset_slug == "market1_No"));
        Ok(())
    }
}

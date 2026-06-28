use crate::errors::YuError;
use crate::sync::sync_server::grpc_sync::PolyMarketHistory;
use li::tools::time::unix_time_now_u64_utc_seconds;
use log::{error, info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use yue::binance::order_book::QueryPayload;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::{query_event_id, query_prices_history, query_series_by_id};
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
async fn split_series_markets(series_id: &str) -> Result<(Vec<MarketWithAddition>, Vec<MarketWithAddition>), YuError> {
    let mut open_markets: Vec<MarketWithAddition> = Vec::new();
    let mut close_markets: Vec<MarketWithAddition> = Vec::new();
    let series = query_series_by_id(series_id, Some(false)).await?;
    let series_id = series.id;
    let series_slug = series.slug;
    let now = unix_time_now_u64_utc_seconds();
    if let Some(events) = series.events {
        trace!("split series_slug:{},events num:{}", series_slug, events.len());
        for event_in_series in events {
            let event_id = event_in_series.id;
            let event_slug = event_in_series.slug;
            match query_event_id(&event_id, None, None).await {
                Ok(event) => {
                    if let Some(markets) = event.markets {
                        trace!("event:{},market num:{}", event_slug, markets.len());
                        for market in markets {
                            // 如果不存在，按照polymarket的尿性,大概率是脏数据了。
                            let start_data = market.start_date.unwrap_or(now + 1);
                            let end_data = market.end_date.unwrap_or(now - 1);
                            let market_with_addition = MarketWithAddition {
                                market: market.clone(),
                                series_id: series_id.clone(),
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

async fn batch_split_series_markets(series_ids: &Vec<String>) -> Result<(Vec<MarketWithAddition>, Vec<MarketWithAddition>), YuError> {
    let mut open_markets: Vec<MarketWithAddition> = vec![];
    let mut close_markets: Vec<MarketWithAddition> = vec![];

    // 并发为每个 series id 运行 split_series_markets
    let mut handles = Vec::with_capacity(series_ids.len());
    for id in series_ids {
        let id_cloned = id.clone();
        handles.push(tokio::spawn(async move { split_series_markets(&id_cloned).await }));
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

/**
1. series_ids下的close market的历史数据Kline数据
2. series_ids下，定时刷新还是运行的market的价格数据，
3. K线的周期，为interval

# polymarket的规则
1. 时间都是到秒。而不是毫秒
2. History的时间戳。一般也不会整点。会慢歌几秒
**/
pub struct SeriesHistoryMarketService {
    series_ids: Vec<String>,
    interval: HistoryInterval,
    open_markets: MarketList,
    history_broadcast: broadcast::Sender<PolyMarketHistory>,
}

impl SeriesHistoryMarketService {
    pub async fn new(series_ids: Vec<String>, interval: HistoryInterval, history_broadcast: broadcast::Sender<PolyMarketHistory>) -> Self {
        let (open_markets, _) = match batch_split_series_markets(&series_ids).await {
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
        }
    }

    pub async fn refresh_open_markets(&self) -> Result<(), YuError> {
        match batch_split_series_markets(&self.series_ids).await {
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

    pub async fn initial_data(&self, start_timestamps: HashMap<&str, u64>) -> Result<(), YuError> {
        let now = self.interval.get_now_close_unix_sec_utc();
        let fidelity = self.interval.to_second() / 60;
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
        Ok(())
    }

    pub async fn query_and_broadcast(&self, query_payload: GetPricesHistoryQuery, asset_index: usize, market: &MarketWithAddition) {
        let asset_id = query_payload.market.clone();
        let asset_slug = match &market.market.outcomes {
            None => {
                format!("{}_{}", market.market.slug, asset_index)
            }
            Some(outcomes) => {
                format!("{}_{}", market.market.slug, outcomes[asset_index])
            }
        };
        let history = query_prices_history(query_payload).await;
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
                    if let Err(e) = self.history_broadcast.send(entry) {
                        error!("asset_slug:{}, timestamp {},history_broadcast error: {:?}", asset_slug, h.t, e);
                    }
                }
            }
            Err(e) => {
                error!("market:{},query prices history error: {:?}", market.market.slug, e);
            }
        }
    }

    pub async fn fetch_last_one_hour_data(&self) -> Result<(), YuError> {
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

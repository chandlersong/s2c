use crate::errors::YuError;
use li::tools::time::unix_time_now_u64_utc_seconds;
use log::{error, info, trace};
use std::sync::Arc;
use tokio::sync::RwLock;
use yue::models::HistoryInterval;
use yue::polymarket::restful_api::{query_event_id, query_series_by_id};
use yue::polymarket::restful_models::Market;

struct MarketWithAddition {
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
**/
pub struct SeriesHistoryMarketService {
    series_ids: Vec<String>,
    interval: HistoryInterval,
    open_markets: MarketList,
}

impl SeriesHistoryMarketService {
    pub async fn new(series_ids: Vec<String>, interval: HistoryInterval) -> Self {
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
        }
    }
}

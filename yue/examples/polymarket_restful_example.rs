use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::http_client::init_http_client;
use yue::polymarket::restful_api::{PolymarketApiTrait, PolymarketClientImpl, query_event_id, query_market_id, query_series_by_id};
use yue::polymarket::restful_models::GetPricesHistoryQuery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("polymarket_restful_example".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let series = query_series_by_id("10151", Some(false)).await?;
    info!("find series:{}", series.slug);

    let event_from_series = series.events.unwrap()[0].clone();

    let event = query_event_id(&event_from_series.id, None, None).await.unwrap();
    info!("find event:{}", event.slug);

    let market_from_event = event.markets.unwrap()[0].clone();
    let market = query_market_id(&market_from_event.id, None).await?;
    info!("find market:{}", market.slug);

    // close marketed，具体token，还是自己去查。
    let token_id = "11291662904897713174667903388388696640643610556195928998276904135282270136756";

    let query = GetPricesHistoryQuery::new(token_id.to_string());
    info!("query: {:?}", query.debug());
    let response = PolymarketClientImpl.query_prices_history(query).await?;
    let history = &response.history;
    info!("received {} Polymarket price history points", history.len());

    for h in history {
        info!("{}: price:{}", unix_2_readable(&(h.t * 1000)), h.p);
    }

    Ok(())
}

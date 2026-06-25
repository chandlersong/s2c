use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::http_client::init_http_client;
use yue::polymarket::restful_api::{query_event_id, query_market_id, query_series_by_id};

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

    Ok(())
}

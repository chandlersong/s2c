use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use rmcp::transport::sse_server::SseServer;
use std::collections::HashMap;
use yu::binance::bn_mcp::BinanceSpot;
use yu::config::get_config;
use yue::http_client::init_http_client;

const BIND_ADDRESS: &str = "127.0.0.1:8000";

#[tokio::main]
async fn main() {
    let app_config = get_config();

    let mut special_log = HashMap::new();
    special_log.insert("mingluan".to_string(), LevelFilter::Info);
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }

    let ct = SseServer::serve(BIND_ADDRESS.parse().unwrap())
        .await
        .unwrap()
        .with_service_directly(BinanceSpot::new);

    tokio::signal::ctrl_c().await.unwrap();
    ct.cancel();
}

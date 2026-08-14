use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use yu::config::get_config;
use yu::errors::YuError;
use yu::okx::duckdb_tables::initial_okx_tables;
use yu::okx::service::OptionService;
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    special_log.insert("okx_option_all_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    initial_okx_tables(None)?;

    let service = Arc::new(OptionService::default());
    service.start().await?;
    let sync_inst_service = service.clone();

    tokio::spawn(async move {
        let _ = sync_inst_service.initial_candle(0, None).await;
    });

    sleep(Duration::from_mins(10)).await;
    Ok(())
}

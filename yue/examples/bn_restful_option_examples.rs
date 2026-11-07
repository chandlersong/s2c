use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yue::binance::option_api::default_binance_option_api;
use yue::errors::YueError;
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YueError> {
    // 配置本地代理（如果需要）
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("bn_restful_option_examples".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let binance_option_api = default_binance_option_api();

    let exchange_info = binance_option_api.get_exchange_info().await?;

    info!(
        "binance server time is:{}, request_weight_limit: {:?}",
        unix_2_readable(&exchange_info.server_time),
        exchange_info.get_request_weight_limit()
    );

    Ok(())
}

use actix::System;
use li::tools::logs::setup_logger;
use log::{error, info, LevelFilter};
use mingluan::binance::jobs::start_bn_jobs;
use mingluan::config::get_config;
use mingluan::errors::MingLuanError;
use std::collections::HashMap;
use yue::http_client::init_http_client;

#[actix::main]
async fn main() -> Result<(), MingLuanError> {
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

    match start_bn_jobs().await {
        Ok(_) => info!("Binance jobs started successfully"),
        Err(e) => {
            error!("Failed to start Binance jobs: {}", e);
            panic!("stop process");
        }
    }
    actix_rt::signal::ctrl_c().await?;
    println!("Received Ctrl+C, shutting down...");
    System::current().stop(); // 优雅停止
    Ok(())
}

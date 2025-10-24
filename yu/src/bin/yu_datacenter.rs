use actix::System;
use li::tools::logs::{parse_level, setup_logger};
use log::{error, info, warn, LevelFilter};
use std::collections::HashMap;
use yu::binance::jobs::start_bn_jobs;
use yu::config::get_config;
use yu::errors::YuError;
use yue::http_client::init_http_client;

#[actix::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let configured_level = parse_level(app_config.log_level.as_deref());
    let mut special_log = HashMap::new();
    error!("Setting app log level to {:?}", configured_level);
    special_log.insert("mingluan".to_string(), configured_level);
    special_log.insert("yue".to_string(), configured_level);
    special_log.insert("li".to_string(), configured_level);

    // Read global log level from config (logLevel). Fallback to Warn if missing/invalid.

    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        info!("don't use proxy");
        init_http_client(None);
    }

    match start_bn_jobs().await {
        Ok(_) => info!("Binance jobs started successfully"),
        Err(e) => {
            error!("Failed to start Binance jobs: {}", e);
            panic!("stop process");
        }
    }

    let shutdown_sender = match yu::arrow_flight_server::start_flight_server("0.0.0.0:8815").await {
        Ok(tx) => {
            info!("Flight server started on 0.0.0.0:8815");
            Some(tx)
        }
        Err(e) => {
            error!("Failed to start Flight server: {}", e);
            None
        }
    };

    // Wait for Ctrl+C in the actix (main) runtime, then signal the flight server to shut down.
    actix_rt::signal::ctrl_c().await?;
    println!("Received Ctrl+C, shutting down...");

    if let Some(tx) = shutdown_sender {
        // Ignore send error: receiver may have already been dropped
        let _ = tx.send(());
    }

    System::current().stop(); // 优雅停止
    Ok(())
}

use actix::System;
use li::tools::logs::setup_logger;
use log::{error, info, LevelFilter};
use std::collections::HashMap;
use yu::binance::jobs::start_bn_jobs;
use yu::config::get_config;
use yu::errors::YuError;
use yue::http_client::init_http_client;

#[actix::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();

    let mut special_log = HashMap::new();
    special_log.insert("mingluan".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    special_log.insert("li".to_string(), LevelFilter::Debug);
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

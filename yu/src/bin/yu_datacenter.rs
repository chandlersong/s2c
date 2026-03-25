use actix::System;
use li::tools::logs::{parse_level, setup_logger};
use log::{error, info, LevelFilter};
use std::collections::HashMap;
use yu::binance::jobs::start_bn_jobs;
use yu::config::get_config;
use yu::data_integrity::jobs::start_check_data_integrity_jobs;
use yue::http_client::init_http_client;

#[actix::main]
async fn main() {
    let app_config = get_config();
    let mut special_log = HashMap::new();

    let log_in_config = app_config.log_level.as_deref();
    special_log.insert("yu_datacenter".to_string(), parse_level(log_in_config));
    special_log.insert("yu".to_string(), parse_level(log_in_config));
    special_log.insert("yue".to_string(), parse_level(log_in_config));
    special_log.insert("li".to_string(), parse_level(log_in_config));
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
    error!(
        "Setting log in config: {:?} ,set to :{:?}",
        log_in_config,
        parse_level(app_config.log_level.as_deref())
    );
    // match start_bn_jobs().await {
    //     Ok(_) => info!("Binance jobs started successfully"),
    //     Err(e) => {
    //         error!("Failed to start Binance jobs: {}", e);
    //         panic!("stop process");
    //     }
    // }
    //
    // match start_check_data_integrity_jobs().await {
    //     Ok(_) => {}
    //     Err(e) => {
    //         error!("Failed to start check data integrity jobs: {}", e);
    //         panic!("stop process");
    //     }
    // }

    match yu::arrow_flight_server::start_flight_server("0.0.0.0:8815").await {
        Ok(()) => {
            info!("✅ Arrow Flight Server successfully started on 0.0.0.0:8815");
        }
        Err(e) => {
            error!("❌ Failed to start Arrow Flight Server: {}", e);
            panic!("Flight server startup failed, aborting");
        }
    }

    // Wait for Ctrl+C in the actix (main) runtime, then signal the flight server to shut down.
    match actix_rt::signal::ctrl_c().await {
        Ok(()) => {
            println!("Received Ctrl+C, shutting down...");
        }
        Err(e) => {
            error!("Signal handler error: {}", e);
        }
    }

    System::current().stop(); // 优雅停止
}

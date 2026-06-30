use li::actix_jobs::AsyncRepeatTask;
use li::tools::logs::{parse_level, setup_logger};
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use yu::binance::jobs::start_bn_jobs;
use yu::config::get_config;
use yu::cron_job;
use yu::data_integrity::clean::TableCleaner;
use yue::http_client::init_http_client;

#[tokio::main(flavor = "multi_thread")]
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
    match start_bn_jobs().await {
        Ok(_) => info!("Binance jobs started successfully"),
        Err(e) => {
            error!("Failed to start Binance jobs: {}", e);
            panic!("stop process");
        }
    }

    let retain_hour = app_config.get_data_retention_hours();
    //clean job
    let _ = cron_job!("0 08 * * * *", move |_uuid, _locked| {
        Box::pin(async move {
            info!("start clean data job");
            let retain_ms = retain_hour * 60 * 60 * 1000;
            let cleaner = TableCleaner::new(retain_ms);
            if let Err(e) = cleaner.execute().await {
                error!("clean data clean: {}", e);
            }
        })
    });

    // Wait for Ctrl+C in the actix (main) runtime, then signal the flight server to shut down.
    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            println!("Received Ctrl+C, shutting down...");
        }
        Err(e) => {
            error!("Signal handler error: {}", e);
        }
    }
}

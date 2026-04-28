// 使用新的 Supervisor + CheckActor 设计的示例
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::binance::bn_data_integrity::{KlineGapRepairStrategy, SpotCheckStrategy, BN_SPOT_KLINE_CHECK};
use yu::config::{get_config, DataIntegrityConfig};
use yu::data_integrity::check::ValidationStrategyTrait;
use yu::data_integrity::models::RepairRequest;
use yu::data_integrity::repair::RepairStrategyTrait;
use yue::http_client::init_http_client;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("data_integrity_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }

    let config = DataIntegrityConfig {
        startup_check_timeout_ms: 5 * 60 * 1000, // 5 mins
        periodic_check_interval_cron: "* 0 * * * * *".to_string(),
        repair_backoff: Default::default(),
    };

    let data_retention_time = app_config.get_data_retention_ms();
    let repair_spot_kline_strategy = KlineGapRepairStrategy::spot();
    let check_spot_kline_strategy = SpotCheckStrategy::spot_check_strategy(None, data_retention_time);
    match check_spot_kline_strategy.validate().await {
        Ok(Some(gaps)) => {
            for g in &gaps.gaps {
                println!("{:?}", g);
            }

            let repair_request = RepairRequest {
                id: 0,
                strategy: "spot".to_string(),
                gaps: gaps.gaps,
            };
            repair_spot_kline_strategy.repair(repair_request).await?;
        }
        Err(_) => {}
        _ => {}
    }
    info!("Data Integrity is complete!");
    Ok(())
}

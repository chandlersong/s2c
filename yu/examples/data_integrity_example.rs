// 使用新的 Supervisor + CheckActor 设计的示例

use actix::Actor;
use async_trait::async_trait;
use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use std::sync::Arc;
use yu::binance::bn_data_integrity::{KlineGapRepairStrategy, SpotCheckStrategy, BN_SPOT_KLINE_CHECK};
use yu::config::{get_config, DataIntegrityConfig};
use yu::data_integrity::check::ValidationStrategy;
use yu::data_integrity::models::{RepairRequest, ValidationGap, ValidationResult};
use yu::data_integrity::repair::RepairStrategy;
use yu::data_integrity::supervisor::DataIntegritySupervisor;
use yu::errors::YuError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;
use yue::tools::get_snow_flake_id_u64;

struct CheckExampleStrategy;

#[async_trait]
impl ValidationStrategy for CheckExampleStrategy {
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError> {
        info!("Validating process");
        Ok(Some(ValidationResult {
            id: get_snow_flake_id_u64(),
            strategy: "example".to_string(),
            gaps: vec![ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 1,
                end_time: 2,
                table: "".to_string(),
            }],
            retry_count: 0,
            error: Some("missing".to_string()),
        }))
    }

    fn name(&self) -> &'static str {
        "example"
    }
}

struct RepairExampleStrategy;

#[async_trait]
impl RepairStrategy for RepairExampleStrategy {
    async fn repair(&self, req: RepairRequest) -> Result<(), String> {
        info!("Repairing: {:?}", req);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "example"
    }
}

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Debug);
    special_log.insert("yu".to_string(), LevelFilter::Debug);
    special_log.insert("data_integrity_example".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
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

    let check_spot_kline_strategy = SpotCheckStrategy::spot_check_strategy(None, data_retention_time);
    // let now = HistoryInterval::FiveMinutes.get_now_close_unix_ms_utc();
    // if let Ok(gaps) = check_spot_kline_strategy.check_one_symbol("BMTUSDT", now) {
    //     println!("Spot check one: {:?}", gaps);
    // }

    let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
    // check_strategies.insert("example".to_string(), Arc::new(CheckExampleStrategy));
    check_strategies.insert(BN_SPOT_KLINE_CHECK.to_string(), Arc::new(check_spot_kline_strategy));

    let repair_spot_kline_strategy = KlineGapRepairStrategy::spot();
    let mut repair_strategies: HashMap<String, Arc<dyn RepairStrategy>> = HashMap::new();
    // repair_strategies.insert("example".to_string(), Arc::new(RepairExampleStrategy));
    repair_strategies.insert(BN_SPOT_KLINE_CHECK.to_string(), Arc::new(repair_spot_kline_strategy));

    let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, repair_strategies).await;
    supervisor.start();
    tokio::time::sleep(std::time::Duration::from_mins(5)).await;
    Ok(())
}

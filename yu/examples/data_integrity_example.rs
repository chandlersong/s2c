// 使用新的 Supervisor + CheckActor 设计的示例

use actix::Actor;
use async_trait::async_trait;
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use std::sync::Arc;
use yu::config::DataIntegrityConfig;
use yu::data_integrity::check::ValidationStrategy;
use yu::data_integrity::models::{RepairRequest, ValidationGap, ValidationResult};
use yu::data_integrity::repair::RepairStrategy;
use yu::data_integrity::supervisor::DataIntegritySupervisor;
use yue::tools::get_snow_flake_id_u64;

struct CheckExampleStrategy;

#[async_trait]
impl ValidationStrategy for CheckExampleStrategy {
    async fn validate(&self) -> ValidationResult {
        info!("Validating process");
        ValidationResult {
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
        }
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

    let config = DataIntegrityConfig {
        startup_check_timeout_ms: 0,
        periodic_check_interval_cron: "*/10 * * * * * *".to_string(),
        repair_backoff: Default::default(),
    };
    let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
    check_strategies.insert("example".to_string(), Arc::new(CheckExampleStrategy));
    let mut repair_strategies: HashMap<String, Arc<dyn RepairStrategy>> = HashMap::new();
    repair_strategies.insert("example".to_string(), Arc::new(RepairExampleStrategy));

    let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, repair_strategies).await;
    supervisor.start();
    tokio::time::sleep(std::time::Duration::from_mins(5)).await;
    Ok(())
}

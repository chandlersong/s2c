use crate::binance::bn_data_integrity::{KlineGapRepairStrategy, SpotCheckStrategy, BN_SPOT_KLINE_CHECK, BN_SWAP_KLINE_CHECK};
use crate::config::get_config;
use crate::data_integrity::check::ValidationStrategy;
use crate::data_integrity::clean::TableCleaner;
use crate::data_integrity::repair::RepairStrategy;
use crate::data_integrity::supervisor::DataIntegritySupervisor;
use crate::errors::YuError;
use actix::Actor;
use li::actix_jobs::CronActor;
use log::info;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn start_check_data_integrity_jobs() -> Result<(), YuError> {
    info!("DataIntegrity starting");
    let app_config = get_config();

    let data_retention_time = app_config.get_data_retention_ms();

    let check_spot_kline_strategy = SpotCheckStrategy::spot_check_strategy(None, data_retention_time);
    let check_swap_kline_strategy = SpotCheckStrategy::swap_check_strategy(None, data_retention_time);
    let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
    check_strategies.insert(BN_SPOT_KLINE_CHECK.to_string(), Arc::new(check_spot_kline_strategy));
    check_strategies.insert(BN_SWAP_KLINE_CHECK.to_string(), Arc::new(check_swap_kline_strategy));
    let repair_spot_kline_strategy = KlineGapRepairStrategy::spot();
    let repair_swap_kline_strategy = KlineGapRepairStrategy::swap();
    let mut repair_strategies: HashMap<String, Arc<dyn RepairStrategy>> = HashMap::new();
    repair_strategies.insert(BN_SPOT_KLINE_CHECK.to_string(), Arc::new(repair_spot_kline_strategy));
    repair_strategies.insert(BN_SWAP_KLINE_CHECK.to_string(), Arc::new(repair_swap_kline_strategy));
    let config = app_config.get_data_integrity_config();

    let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, repair_strategies).await;
    let _ = supervisor.start();
    info!("DataIntegrity started successfully");

    let cleaner = TableCleaner::new(app_config.get_data_retention_ms());
    //FUTURE: 可配置
    let _ = CronActor::new("0 12 */3 * * * *", cleaner).start();
    info!("data cleaner started successfully");
    Ok(())
}

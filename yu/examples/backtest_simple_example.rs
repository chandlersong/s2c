use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use yu::backtest::binance::pg_datafeed::SpotHourlyDataFeed;
use yu::backtest::core::EngineTrait;
use yu::backtest::selection_strategy::demo::SelectionDemoStrategy;
use yu::backtest::selection_strategy::{BatchSelectionEngine, position_summary_lines};
use yu::config::get_config;
use yu::errors::YuError;
use yu::postgresql_db::get_sync_client_pg_pool_sync;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("backtest_simple_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log)?;
    let _app_config = get_config();

    let pg_pool = get_sync_client_pg_pool_sync()?;
    let data_feed = SpotHourlyDataFeed::new(0, pg_pool, 5000);

    let demo_strategy = SelectionDemoStrategy::new_for_demo();

    let engine = BatchSelectionEngine::new(Box::new(data_feed), demo_strategy);

    let result = engine.execute().await?;

    info!("total rows: {}", result.height());

    Ok(())
}

use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use polars::io::ipc::IpcWriter;
use polars::prelude::SerWriter;
use std::collections::HashMap;
use yu::backtest::binance::pg_datafeed::SpotHourlyDataFeed;
use yu::backtest::core::VectorDataFeedTrait;
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
    let mut df = data_feed.get_all_data().await?;
    info!(
        "DataFrame shape: {:?},memory usage: {:.2} MB",
        df.shape(),
        df.estimated_size() as f64 / 1024.0 / 1024.0
    );
    let mut buf: Vec<u8> = Vec::new();
    IpcWriter::new(&mut buf).finish(&mut df)?;
    println!("Parquet MB: {:.2}", buf.len() as f64 / 1024.0 / 1024.0);
    Ok(())
}

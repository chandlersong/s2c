use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use log::{LevelFilter, error, info};
use std::collections::HashMap;
use yu::config::get_config;
use yu::data_integrity::check::{SyncClientBinarySearchDataImpl, binary_search_gap};
use yu::data_integrity::models::ValidationGap;
use yu::errors::YuError;
use yu::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool};
use yu::sync::client::db_consts::ClientsTables;
use yue::models::HistoryInterval;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let _app_config = get_config();
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("sync_client_binary_check_example".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    let pg_pool = get_sync_client_pg_pool().await?;
    let okx_binary_search_ds = SyncClientBinarySearchDataImpl::new(
        pg_pool.clone(),
        ClientsTables::OkxPriceHistory.table_name(),
        ClientsTables::OkxInstruments.table_name(),
        "candle_begin_time",
    );
    let interval = HistoryInterval::OneHour;
    let start = interval.get_close_unix_ms(1778833800000) + interval.to_milliseconds();
    //测试数据，从数据库里面那吧
    let gaps: Vec<ValidationGap> = binary_search_gap(
        "632749436359869363",
        "SPOT",
        start,
        interval.get_now_close_unix_ms_utc(),
        &interval,
        okx_binary_search_ds.clone(),
    )?;
    info!("gaps: {:?}", gaps.len());
    for gap in gaps {
        match gap {
            ValidationGap::MissingData { start_time, end_time, .. } => {
                info!("MissingData from: {} to {}", unix_2_readable(&start_time), unix_2_readable(&end_time));
            }
            _ => {
                error!("should not happen, gap: {:?}", gap);
            }
        }
    }

    Ok(())
}

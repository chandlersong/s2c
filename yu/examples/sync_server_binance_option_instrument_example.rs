use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::sync_job::start_binance_sync_server_job;
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::models::grpc_sync::{InstrumentType, instrument};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("sync_server_binance_option_instrument_example".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let app_config = get_config();
    init_http_client(app_config.proxy_url.as_deref());
    let (history_tx, mut history_rx) = mpsc::channel(1000);
    let _history_receiver = tokio::spawn(async move { while history_rx.recv().await.is_some() {} });

    let instrument_service = start_binance_sync_server_job(&app_config, history_tx).await?;
    let synced_instruments = instrument_service.sync_instrument().await?;
    let synced_option_count = synced_instruments
        .iter()
        .filter(|instrument| instrument.instrument_type == InstrumentType::BinanceOption as i32)
        .count();
    info!("Synced Binance Option instruments: {synced_option_count}");

    let listed_instruments = instrument_service.list_instruments().await?;
    for instrument in listed_instruments {
        if let Some(instrument::Payload::BinanceOptionInstrument(option)) = instrument.payload {
            info!("Binance Option instrument: {} ({})", option.symbol, option.status);
        }
    }

    info!("Binance Option instrument sync is running; press Ctrl+C to stop");
    tokio::signal::ctrl_c()
        .await
        .map_err(|error| YuError::new(&format!("failed to wait for Ctrl+C: {error}")))?;
    Ok(())
}

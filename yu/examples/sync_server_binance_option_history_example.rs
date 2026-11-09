use li::tools::logs::setup_logger;
use log::{LevelFilter, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use yu::binance::db_consts::initial_tables;
use yu::binance::sync_server::{
    BinanceOptionKlineSyncHistoryService, BinanceOptionMarketPriceSyncHistoryService, BinanceOptionSyncInstrumentService,
};
use yu::config::get_config;
use yu::errors::YuError;
use yu::sync::server::sync_server::{SyncHistoryServiceTrait, SyncInstrumentServiceTrait};
use yue::http_client::init_http_client;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let mut special_log = HashMap::new();
    special_log.insert("sync_server_binance_option_history_example".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let app_config = get_config();
    init_http_client(app_config.proxy_url.as_deref());
    let binance_config = app_config
        .binance
        .as_ref()
        .filter(|config| config.enable())
        .ok_or_else(|| YuError::ConfigError("Binance sync server is not configured or disabled".to_string()))?;
    let interval = binance_config.get_kline_interval()?;

    initial_tables(None)?;
    let (history_tx, mut history_rx) = mpsc::channel(1000);
    let _history_receiver = tokio::spawn(async move { while history_rx.recv().await.is_some() {} });

    let instrument_service = BinanceOptionSyncInstrumentService::for_production();
    let instruments = instrument_service.sync_instrument().await?;
    info!("Synced {} Binance Option instruments", instruments.len());

    let live_instruments = instrument_service.live_instruments();
    let kline_service = BinanceOptionKlineSyncHistoryService::for_production(
        app_config.get_data_retention_ms(),
        live_instruments.clone(),
        history_tx.clone(),
        interval,
    );
    let market_price_service = BinanceOptionMarketPriceSyncHistoryService::for_production(live_instruments, history_tx);

    kline_service.initial_history_data(None).await?;
    market_price_service.initial_history_data(None).await?;
    kline_service.check_history_data(None).await?;
    market_price_service.check_history_data(None).await?;
    kline_service.sync_all().await?;
    market_price_service.sync_all().await?;

    info!("Binance Option history sync completed");
    Ok(())
}

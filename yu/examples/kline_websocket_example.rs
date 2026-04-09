use actix::fut::err;
use li::tools::logs::setup_logger;
use log::{error, LevelFilter};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::watch;
use yu::binance::bn_backend_service::get_spot_kline_table;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::websocket_service::KlineSubscribeService;
use yu::config::get_config;
use yu::errors::YuError;
use yue::binance::bn_models::common::SymbolType;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Debug);
    special_log.insert("yu".to_string(), LevelFilter::Debug);
    special_log.insert("kline_websocket_example".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();

    let symbol_type = SymbolType::Spot;
    let spot_kline = get_spot_kline_table();
    let proxy = app_config.proxy_url.clone();
    let dash_board = Arc::new(BinanceDashboard::debug_mode(app_config.get_data_retention_hours()));
    let snapshot = dash_board.execute().await?;
    let (dash_board_watch, _) = watch::channel(snapshot);
    if let Err(e) = KlineSubscribeService::startup(symbol_type, spot_kline, dash_board_watch, proxy).await {
        error!("error starting kline service: {}", e);
    }

    tokio::time::sleep(tokio::time::Duration::from_mins(30)).await;

    Ok(())
}

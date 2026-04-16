use li::tools::logs::setup_logger;
use log::{info, warn, LevelFilter};
use std::collections::HashMap;
use yu::binance::bn_backend_service::get_raw_spot_kline_table;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history::initial_spot_kline;
use yu::binance::jobs::initial_tables;
use yu::config::get_config;
use yu::errors::YuError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

#[tokio::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }

    let mut special_log = HashMap::new();
    special_log.insert("yu".to_string(), LevelFilter::Debug);
    special_log.insert("li".to_string(), LevelFilter::Debug);
    special_log.insert("kline_history_example".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    if let Err(_e) = initial_tables(None) {
        warn!("币安表创建失败,{}", _e);
    }
    //初始化数据
    let dash_board = BinanceDashboard::debug_mode(app_config.get_data_retention_hours());
    dash_board.execute().await?;
    let spot_all = dash_board.spot_all_symbols();
    let spot_symbol: Vec<String> = spot_all
        .read()
        .unwrap()
        .iter()
        .filter(|s| s.quote_asset == "USDT")
        .map(|s| s.symbol.clone())
        .collect();
    let db = get_raw_spot_kline_table();
    initial_spot_kline(spot_symbol, app_config, HistoryInterval::OneHour, db).await?;

    Ok(())
}

use li::tools::logs::setup_logger;
use log::{LevelFilter, info, warn};
use std::collections::HashMap;
use tokio::sync::watch;
use yu::binance::bn_dashboard::BinanceDashboard;
use yu::binance::history::start_sync_funding_rate;
use yu::binance::jobs::initial_tables;
use yu::config::get_config;
use yu::errors::YuError;
use yue::http_client::init_http_client;
use yue::models::HistoryInterval;

/// 建立这个例子，主要是在初始化的时候，发现GRASSUSDT一直取不到数据
/// 所以也就在这里用了一下
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
    special_log.insert("mingluan".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    if let Err(_e) = initial_tables(None) {
        warn!("币安表创建失败,{}", _e);
    }
    #[allow(deprecated)]
    let dash_board = BinanceDashboard::debug_mode(app_config.get_data_retention_hours());
    let snapshot = dash_board.execute().await?;
    let (dash_board_watch, _) = watch::channel(snapshot);
    let swap_all = dash_board.swap_all_symbols();
    let swap_symbol: Vec<String> = swap_all
        .read()
        .unwrap()
        .iter()
        .filter(|s| s.quote_asset == "USDT")
        .map(|s| s.symbol.clone())
        .collect();
    start_sync_funding_rate(swap_symbol, app_config, HistoryInterval::OneHour, dash_board_watch)
        .await
        .unwrap();
    Ok(())
}

use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::config::get_config;
use yu::errors::YuError;
use yue::http_client::init_http_client;

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

    Ok(())
}

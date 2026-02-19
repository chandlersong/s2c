use actix::Actor;
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use std::time::Duration;
use yu::config::{get_config, AccountConfig};
use yu::errors::YuError;
use yue::binance::bn_restful_commands::SWAP_LISTEN_KEY_COMMAND;
use yue::binance::listen_key_client::ListenKeyClient;
use yue::http_client::init_http_client;

#[actix::main]
async fn main() -> Result<(), YuError> {
    let app_config = get_config();
    let mut special_log = HashMap::new();
    special_log.insert("li".to_string(), LevelFilter::Debug);
    special_log.insert("yu".to_string(), LevelFilter::Debug);
    special_log.insert("swap_listen_key_example".to_string(), LevelFilter::Debug);
    special_log.insert("yue".to_string(), LevelFilter::Debug);
    setup_logger(Some(LevelFilter::Warn), special_log).unwrap();
    let proxy = app_config.proxy_url.clone();
    if let Some(url_proxy) = proxy {
        info!("Using proxy: {}", url_proxy);
        init_http_client(Some(&url_proxy));
    } else {
        init_http_client(None);
    }

    // 从配置中安全获取第一个 HMAC 类型的账户
    let account = app_config
        .binance
        .as_ref()
        .unwrap()
        .accounts
        .as_ref()
        .unwrap()
        .iter()
        .find_map(|a| match a {
            AccountConfig::HMAC { .. } => Some(a.clone()), // 直接克隆并返回整个对象
            _ => None,
        });

    if let Some(AccountConfig::HMAC {
        account_name,
        api_key,
        api_secret,
    }) = account
    {
        let _ = ListenKeyClient::swap(
            &account_name,
            SWAP_LISTEN_KEY_COMMAND.clone(),
            SWAP_LISTEN_KEY_COMMAND.clone(),
            None,
            &api_key,
            &api_secret,
            app_config.proxy_url.clone(),
        )
        .start();
    }

    loop {
        tokio::time::sleep(Duration::from_millis(5 * 60 * 1000)).await;
    }
}

use actix::{Actor, Handler};
use li::subscribe_event_addr;
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use std::time::Duration;
use yu::config::{get_config, SecurityType};
use yu::errors::YuError;
use yue::binance::bn_models::swap_account_stream::BinanceSwapAccountStreamResponse;
use yue::binance::bn_restful_commands::SWAP_LISTEN_KEY_COMMAND;
use yue::binance::listen_key_client::ListenKeyClient;
use yue::http_client::init_http_client;

struct SwapAccountPrinter;

impl Actor for SwapAccountPrinter {
    type Context = actix::Context<Self>;
}

impl Handler<BinanceSwapAccountStreamResponse> for SwapAccountPrinter {
    type Result = ();

    fn handle(&mut self, msg: BinanceSwapAccountStreamResponse, _: &mut Self::Context) -> Self::Result {
        info!("received Binance SwapAccountStream:{:?}", msg);
    }
}

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
    let account = app_config.binance.as_ref().unwrap().accounts.as_ref().unwrap().iter().find_map(|a| {
        if a.secret_type == SecurityType::HMAC {
            info!("账户{}开始监听", a.account_name);
            Some(a.clone())
        } else {
            None
        }
    });

    if let Some(acc) = account {
        let addr = ListenKeyClient::swap(
            &acc.account_name,
            SWAP_LISTEN_KEY_COMMAND.clone(),
            SWAP_LISTEN_KEY_COMMAND.clone(),
            None,
            &acc.api_key,
            &acc.value,
            app_config.proxy_url.clone(),
        )
        .start();
        let printer_addr = SwapAccountPrinter.start();
        subscribe_event_addr!(addr, printer_addr, BinanceSwapAccountStreamResponse);
    }

    loop {
        tokio::time::sleep(Duration::from_millis(5 * 60 * 1000)).await;
    }
}

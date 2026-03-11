use actix::{Actor, Context, Handler};
use li::subscribe_event_addr;
use li::tools::logs::setup_logger;
use li::websocket::client::{WebSocketClient, WebSocketEvent};
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::binance::jobs::initial_tables;
use yu::config::{get_config, SecurityType};
use yu::websocket::subscribers::AccountSyncActor;
use yue::binance::bn_json_websocket::SPOT_WEBSOCKET;
use yue::binance::bn_models::common::SpotOrderData;
use yue::binance::bn_models::spot_websocket::BinanceSpotAccountWebSocketResponse;
use yue::binance::websocket_actor::SpotAccountActor;

struct PrintActor {
    name: String,
}

impl PrintActor {
    fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

impl Actor for PrintActor {
    type Context = Context<Self>;
}

impl Handler<BinanceSpotAccountWebSocketResponse> for PrintActor {
    type Result = ();

    fn handle(&mut self, event: BinanceSpotAccountWebSocketResponse, _ctx: &mut Self::Context) {
        match event {
            BinanceSpotAccountWebSocketResponse::OutboundAccountPosition(e) => {
                info!("Account {} received OutboundAccountPosition event: {:?}", self.name, e);
            }
            BinanceSpotAccountWebSocketResponse::BalanceUpdate(e) => {
                info!("Account {} received BalanceUpdate event: {:?}", self.name, e);
            }
            BinanceSpotAccountWebSocketResponse::ExecutionReport(e) => {
                info!("Account {} received ExecutionReport event: {:?}", self.name, e);
            }
            BinanceSpotAccountWebSocketResponse::SubscribeResponse(e) => {
                info!("Account {} received SubscribeResponse event: {:?}", self.name, e);
            }
        }
    }
}
#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger(
        Some(LevelFilter::Warn),
        HashMap::from([
            ("yu".to_string(), LevelFilter::Trace),
            ("yue".to_string(), LevelFilter::Trace),
            ("li".to_string(), LevelFilter::Trace),
            ("bn_account_websocket_example".to_string(), LevelFilter::Trace),
        ]),
    )
    .expect("日志初始化失败");

    let client_addr = WebSocketClient::new(SPOT_WEBSOCKET)
        .with_proxy("http://127.0.0.1:7891")
        .with_reconnect_interval(std::time::Duration::from_secs(10))
        .start();

    let _ = initial_tables(None);
    info!("WebSocket 客户端已启动（环境变量代理）");

    let acc_infos = get_config()
        .binance
        .as_ref()
        .and_then(|ws| ws.accounts.as_ref())
        .map(|accounts| {
            accounts
                .iter()
                .filter_map(|acc| {
                    if acc.secret_type == SecurityType::Ed25519 {
                        info!("账户{}开始监听", acc.account_name);
                        Some(acc.clone().into())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let handler = SpotAccountActor::new(acc_infos).start();
    let printer = PrintActor::new("MainPrinter").start();
    let account_sync_add = AccountSyncActor::new(None).start();

    subscribe_event_addr!(client_addr, handler.clone(), BinanceSpotAccountWebSocketResponse);
    subscribe_event_addr!(client_addr, handler.clone(), WebSocketEvent);
    subscribe_event_addr!(client_addr, printer, BinanceSpotAccountWebSocketResponse);
    subscribe_event_addr!(handler.clone(), account_sync_add, SpotOrderData);

    tokio::signal::ctrl_c().await?;
    Ok(())
}

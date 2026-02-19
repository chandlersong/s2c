use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::binance::jobs::initial_tables;
use yu::config::{get_config, SecurityType};
use yu::websocket::subscribers::AccountSyncActor;
use yue::binance::bn_json_websocket::SPOT_WEBSOCKET;
use yue::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use yue::binance::websocket_handler::SpotAccountStreamHandler;
use yue::websocket::client::{SubscribeToEvents, WebSocketClient, WebSocketEvent};
use yue::websocket::event_bus::{Subscribe, WsMessageBus};

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

impl Handler<BinanceSpotWebSocketResponse> for PrintActor {
    type Result = ();

    fn handle(&mut self, event: BinanceSpotWebSocketResponse, _ctx: &mut Self::Context) {
        match event {
            BinanceSpotWebSocketResponse::OutboundAccountPosition(e) => {
                info!("Account {} received OutboundAccountPosition event: {:?}", self.name, e);
            }
            BinanceSpotWebSocketResponse::BalanceUpdate(e) => {
                info!("Account {} received BalanceUpdate event: {:?}", self.name, e);
            }
            BinanceSpotWebSocketResponse::ExecutionReport(e) => {
                info!("Account {} received ExecutionReport event: {:?}", self.name, e);
            }
            BinanceSpotWebSocketResponse::SubscribeResponse(e) => {
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

    let handler = SpotAccountStreamHandler::new(acc_infos);
    let bus = WsMessageBus::new(handler).start();
    let printer = PrintActor::new("MainPrinter").start();
    let account_sync_add = AccountSyncActor::new(None).start();

    bus.do_send(Subscribe {
        subscriber: printer.recipient(),
    });
    bus.do_send(Subscribe {
        subscriber: account_sync_add.recipient(),
    });

    info!("✓ WsMessageBus started");

    client_addr
        .send(SubscribeToEvents {
            recipient: bus.recipient::<WebSocketEvent>(),
        })
        .await??;

    tokio::signal::ctrl_c().await?;
    Ok(())
}

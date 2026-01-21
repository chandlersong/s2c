use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::config::get_config;
use yue::binance::bn_json_websocket::SPOT_WEBSOCKET;
use yue::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use yue::binance::websocket_handler::SpotAccountStreamHandler;
use yue::websocket::client::{SubscribeToEvents, WebSocketClient, WebSocketEvent};
use yue::websocket::event_bus::WsMessageBus;

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

    info!("WebSocket 客户端已启动（环境变量代理）");

    let acc_infos = get_config()
        .binance_websocket
        .as_ref()
        .and_then(|ws| ws.spot.as_ref())
        .map(|spot| spot.accounts.iter().map(|acc| acc.clone().into()).collect())
        .unwrap_or_default();

    let handler = SpotAccountStreamHandler::new(acc_infos);
    let bus = WsMessageBus::new(handler).start();
    let printer = PrintActor::new("MainPrinter").start();

    bus.do_send(yue::websocket::event_bus::Subscribe {
        subscriber: printer.recipient(),
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

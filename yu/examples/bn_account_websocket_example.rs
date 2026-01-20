use actix::{Actor, Context, Handler};
use li::tools::logs::setup_logger;
use li::tools::time::unix_time_now_u64_utc;
use log::{info, LevelFilter};
use std::collections::HashMap;
use yu::config::get_config;
use yue::binance::bn_json_websocket::{CommandRequest, SPOT_WEBSOCKET, USER_DATA_STREAM_SUBSCRIBE_SIGNATURE};
use yue::binance::bn_models::spot_websocket::BinanceSpotWebSocketResponse;
use yue::binance::parsers::SpotAccountStreamParser;
use yue::tools::{load_ed25519_signing_key, sign_ed25519, SnowyFlakeWrapper};
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
    let mut special_log = HashMap::new();
    special_log.insert("yu".to_string(), LevelFilter::Trace);
    special_log.insert("yue".to_string(), LevelFilter::Trace);
    special_log.insert("li".to_string(), LevelFilter::Trace);
    special_log.insert("bn_account_websocket_example".to_string(), LevelFilter::Trace);
    setup_logger(Some(LevelFilter::Warn), special_log).expect("TODO: panic message");

    let client_addr = WebSocketClient::new(SPOT_WEBSOCKET)
        .with_proxy("http://127.0.0.1:7891")
        .with_reconnect_interval(std::time::Duration::from_secs(10))
        .start();

    info!("WebSocket 客户端已启动（环境变量代理）");

    let app_config = get_config();
    let mut id_to_name = HashMap::new();
    let now = unix_time_now_u64_utc();
    let snow_flake_generate = SnowyFlakeWrapper::new();

    if let Some(websocket_config) = &app_config.binance_websocket {
        if let Some(spot_websocket) = &websocket_config.spot {
            for acc in &spot_websocket.accounts {
                let mut private_key = load_ed25519_signing_key(acc.secret_key.as_ref())?;
                let payload = format!("apiKey={}&timestamp={}", acc.api_key, now);
                let signature = sign_ed25519(payload, &mut private_key)?;
                let id = snow_flake_generate.next_id_u64();
                let param = HashMap::from([
                    ("signature".to_string(), signature),
                    ("apiKey".to_string(), acc.api_key.clone()),
                    ("timestamp".to_string(), now.to_string()),
                ]);

                id_to_name.insert(id, acc.account_name.clone());
                let command = CommandRequest {
                    method: USER_DATA_STREAM_SUBSCRIBE_SIGNATURE.to_string(),
                    params: param,
                    id,
                };
                client_addr
                    .send(yue::websocket::client::SendTextMessage::new(serde_json::to_string(&command)?))
                    .await??;
            }
        }
    }
    let parser = SpotAccountStreamParser::new(id_to_name);
    let bus = WsMessageBus::new(parser).start();
    let printer = PrintActor::new("MainPrinter").start();
    bus.do_send(yue::websocket::event_bus::Subscribe {
        subscriber: printer.clone().recipient(),
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

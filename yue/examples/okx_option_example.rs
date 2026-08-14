use async_trait::async_trait;
use li::tools::logs::setup_logger;
use li::tools::time::unix_2_readable;
use li::websocket::connection::{CommandMessage, MessageHandlerTrait, ToServerMessage, WebSocketConnection};
use log::{LevelFilter, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use yue::http_client::init_http_client;
use yue::okx::models::websocket::{ArgBody, OkxWebsocketResponse};
use yue::okx::restful_api::{HistoryParams, default_okx_api, list_okx_option};
use yue::okx::websocket_channel::{CommandRequest, OXK_BUSINESS_WEBSOCKET};
struct PrinterMessageHandler {}
#[async_trait]
impl MessageHandlerTrait<OkxWebsocketResponse> for PrinterMessageHandler {
    async fn handle_message(&self, message: &OkxWebsocketResponse) {
        match message {
            OkxWebsocketResponse::SubscribeResponse(m) => {
                info!("response subscribed:{:?}", m);
            }
            OkxWebsocketResponse::Kline(kline) => {
                info!("kline: {:?}", kline);
            }
        };
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proxy = Option::from("http://localhost:7891");
    init_http_client(proxy);
    let mut special_log = HashMap::new();
    special_log.insert("yue".to_string(), LevelFilter::Info);
    special_log.insert("li".to_string(), LevelFilter::Info);
    special_log.insert("okx_option_example".to_string(), LevelFilter::Info);
    setup_logger(Some(LevelFilter::Warn), special_log)?;

    let btc_option = list_okx_option("BTC-USD").await?;
    info!("find {} options for BTC-USD", btc_option.data.len());
    for data in &btc_option.data {
        let exp_time = unix_2_readable(&data.exp_time.unwrap_or(0));
        info!("find {}, exprie at {}", data.inst_id, exp_time);
    }
    let last_one = &btc_option.data[100];
    info!("try to query {} history", last_one.inst_id);

    let api = default_okx_api();
    let query_param = HistoryParams::new_only_inst_1h(last_one.inst_id.to_string());
    let history = api.query_history_candle(query_param).await?;
    info!("history contains {} candles", history.data.len());
    let first_candle = history.data.first().unwrap();
    let last_candle = history.data.last().unwrap();
    info!(
        "candle begin is from {} to {}",
        unix_2_readable(&first_candle[0].parse::<u64>().unwrap_or(0)),
        unix_2_readable(&last_candle[0].parse::<u64>().unwrap_or(0))
    );

    let all_btc_option = btc_option.data;
    tokio::spawn(async move {
        let proxy = Some("http://127.0.0.1:7891".to_string());
        let handler = Arc::new(PrinterMessageHandler {});
        let interface = WebSocketConnection::run::<OkxWebsocketResponse>(OXK_BUSINESS_WEBSOCKET.to_string(), None, proxy, Some(handler)).await;
        info!("✓ WebSocket 客户端已启动");
        let mut args = vec![];
        for inst in all_btc_option.iter() {
            let arg = ArgBody::builder()
                .channel("candle1m".to_string())
                .inst_id(inst.inst_id.to_string())
                .build();
            args.push(arg);
        }

        let request = CommandRequest::builder()
            .id("1".to_string())
            .op("subscribe".to_string())
            .args(args)
            .build();
        let command_test = serde_json::to_string(&request).unwrap();
        interface.send_command(CommandMessage::ToServer(ToServerMessage::text(command_test)));
    });
    tokio::time::sleep(Duration::from_mins(5)).await;

    Ok(())
}

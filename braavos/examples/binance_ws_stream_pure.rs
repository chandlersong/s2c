use braavos::binance::bn_models::WsMethod::SUBSCRIBE;
use braavos::binance::bn_ws_commands::{WsRequest, WsSpotResponse};
use braavos::utils::setup_logger;
use futures_util::{SinkExt, StreamExt};
use log::{error, info, LevelFilter};
use std::sync::Arc;
use tokio::sync::Barrier;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let url = "wss://stream.binance.com:9443/ws/bbb";
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    info!("WebSocket handshake has been successfully completed");
    let (mut write, mut read) = ws_stream.split();

    // let subscribe_message = r#"{ "method": "SUBSCRIBE", "params": ["btcusdt@aggTrade"], "id": 1 }"#;
    // let params = Some(vec!["btcusdt@aggTrade".to_string(), "btcusdt@depth".to_string()]);
    let params = Some(vec!["btcusdt@depth".to_string()]);
    let subscribe_request = WsRequest::new(SUBSCRIBE, params);
    let request_body = subscribe_request.to_json();
    println!("request body is {}", request_body);
    let msg = Message::Text(request_body);
    if let Err(e) = write.send(msg).await {
        eprintln!("Error while sending message: {}", e);
    }

    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();

    tokio::spawn(async move {
        while let Some(message) = read.next().await {
            if let Ok(msg) = message {
                match msg {
                    Message::Text(txt) => {
                        info!("Received: {}", txt);
                        let entity: WsSpotResponse = serde_json::from_str(&txt).unwrap();
                        match entity {
                            WsSpotResponse::Depth(v) => {
                                info!("{:?} at {:?}", v.symbol,v.event_time);
                            }
                            a => {
                                error!("Received unexpected: {:?}", a);
                            }
                        }

                    }
                    Message::Ping(ping) => {
                        // Respond to Ping messages with Pong
                        let ping_text = String::from_utf8_lossy(&ping).to_string();
                        info!("收到ping消息:{:?}", ping_text);
                        if write.send(Message::Pong(ping)).await.is_err() {
                            info!("Failed to send Pong");
                            return;
                        }
                        info!("发送pong消息");
                    }
                    Message::Pong(_) => {
                        // Optionally handle Pong messages
                        println!("Received Pong");
                    }
                    Message::Close(_) => {
                        // Handle close messages if needed
                        info!("Received Close message");
                        barrier_clone.wait().await;
                        return;
                    }
                    a => {
                        info!("收到其他消息,{:?}",a);
                    }
                }
            }
        }
    });
    // 主线程等待 Barrier
    barrier.wait().await;
    println!("主线程收到信号,继续执行");
}



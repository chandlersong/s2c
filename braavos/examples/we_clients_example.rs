use braavos::tools::setup_logger;
use braavos::websockets::WebSocketClient;
use log::LevelFilter;
use std::sync::Arc;
use tokio::sync::Barrier;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let url = "wss://fstream.binance.com/ws/spot";
    let client = WebSocketClient::new(url, None).await.unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let subscribe = r#"{"method": "SUBSCRIBE", "params": ["btcusdt@ticker"], "id": 1}"#;
    client.send(Message::Text(subscribe.to_string())).await.unwrap();

    barrier.wait().await;
}
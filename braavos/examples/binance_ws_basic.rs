use braavos::binance::bn_models::WsMethod::Time;
use braavos::binance::bn_ws_commands::WsRequest;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let url = "wss://ws-api.binance.com:443/ws-api/v3";
    // 创建一个 reqwest 客户端并设置代理
    let ping_request = WsRequest::empty_new(Time);

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");
    let (mut write, mut read) = ws_stream.split();
    //
    // let subscribe_message = r#"{ "method": "SUBSCRIBE", "params": ["btcusdt@aggTrade"], "id": 1 }"#;

    let request_body = ping_request.to_json();
    println!("request body is {}", request_body);
    let msg = Message::Text(request_body);
    if let Err(e) = write.send(msg).await {
        eprintln!("Error while sending message: {}", e);
    }
    //
    while let Some(message) = read.next().await {
        match message {
            Ok(msg) => println!("Received: {}", msg),
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}



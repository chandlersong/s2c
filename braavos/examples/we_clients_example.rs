use braavos::binance::bn_models::SpotWsSubscribe::AllMiniTicker;
use braavos::binance::bn_models::WsMethod::SUBSCRIBE;
use braavos::binance::bn_ws_commands::WsRequest;
use braavos::tools::setup_logger;
use braavos::websockets::WebSocketClient;
use log::{info, LevelFilter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let url = "wss://fstream.binance.com/ws/spot";
    let client = WebSocketClient::new(url, None).await.unwrap();
    let params: Option<Vec<String>> = Some(vec![String::from(AllMiniTicker)]);
    let subscribe_request = WsRequest::new(SUBSCRIBE, params);
    client.send(subscribe_request.to_ws_message()).await.unwrap();

    let mut rx = client.subscribe_text_message_sender().await;

    tokio::spawn(async move {
        loop {
            if let Ok(msg) = rx.recv().await {
                info!("Received message: {:?}", msg);
            }
        }
    });
    loop {
        sleep(Duration::from_secs(10)).await;
        info!("运行了10s")
    }
}

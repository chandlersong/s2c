use braavos::binance::bn_models::WsMethod::SUBSCRIBE;
use braavos::binance::bn_ws_commands::{BinanceWSClient, WsRequest};
use braavos::utils::setup_logger;
use log::LevelFilter;
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let mut ws_client = BinanceWSClient::connect_and_listen().await;


    let barrier = Arc::new(Barrier::new(2));

    let params = Some(vec!["btcusdt@depth".to_string()]);
    let subscribe_request = WsRequest::new(SUBSCRIBE, params);
    ws_client.send_command(subscribe_request).await.expect("message send failed");
    barrier.wait().await;
}

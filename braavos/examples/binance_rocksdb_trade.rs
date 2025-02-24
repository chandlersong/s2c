use braavos::binance::bn_dashboard::get_spot_client;
use braavos::binance::bn_models::get_trade_command;
use braavos::binance::bn_models::WsMethod::SUBSCRIBE;
use braavos::binance::bn_ws_commands::WsRequest;
use braavos::tools::setup_logger;
use log::{debug, LevelFilter};
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let mut ws_client = get_spot_client().await;
    let params: Option<Vec<String>> = Some(vec![
        get_trade_command("BTCUSDT"),
    ]);
    let subscribe_request = WsRequest::new(SUBSCRIBE, params);
    ws_client.send_command(subscribe_request).await.expect("subscribe spot mini ticker failed");
    let tx = ws_client.get_trade_tx(&"BTCUSDT".to_string()).await;
    let barrier = Arc::new(Barrier::new(2));
    tokio::spawn(async move {
        debug!("start receive data");
        let mut rx = tx.subscribe();
        loop {
            let ticker = rx.recv().await;

            if let Ok(t) = ticker {
                debug!("receive data: {:?}", t);
            }
        }
    });
    barrier.wait().await;
}

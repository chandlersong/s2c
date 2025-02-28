use braavos::binance::bn_dashboard::get_spot_client;
use braavos::tools::setup_logger;
use log::{debug, info, LevelFilter};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let mut ws_client = get_spot_client().await;
    let mut rx = ws_client.subscribe_trade("BTCUSDT").await;
    tokio::spawn(async move {
        debug!("start receive data");
        loop {
            let ticker = rx.recv().await;

            if let Ok(t) = ticker {
                debug!("receive data: {:?}", t);
            }
        }
    });
    let mut minutes = 1;
    loop {
        sleep(Duration::from_secs(60)).await;
        info!("运行了{}分钟",minutes);
        minutes = minutes + 1;
    }
}

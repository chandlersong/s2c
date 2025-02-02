use braavos::binance::bn_models::BinanceBase;
use braavos::binance::bn_models::WsMethod::GetProperty;
use braavos::binance::bn_ws_commands::{connect_and_listen, WsRequest};
use braavos::cache::{DashBoard, FrequencyDashBoard};
use braavos::tools::setup_logger;
use log::{debug, info, LevelFilter};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;
use tokio::time::sleep;

///
/// 这个用例，主要是为了演示一个websocket启动的监听过程。
/// 整个过程应该
/// 1. 准备应对各种消息的处理。
///     - 缓存的初始化。
/// 2. 开启监听。
/// 3. 发送命令。
///
///

#[tokio::main]
async fn main() {
    let _ = setup_logger(Some(LevelFilter::Debug));
    let url = format!("{}/stream?streams=!miniTicker@arr", String::from(BinanceBase::WsSwapStreamUrl));
    let mut ws_client = connect_and_listen(url).await;


    let barrier = Arc::new(Barrier::new(2));

    let params = Some(vec!["combined".to_string()]);
    let subscribe_request = WsRequest::new(GetProperty, params);
    ws_client.send_command(subscribe_request).await.expect("message send failed");


    let mut listener = ws_client.mini_ticker_tx.subscribe();
    let dashboard = FrequencyDashBoard::new(1000).await;
    let mut dashboard_read = dashboard.clone();
    tokio::spawn(async move {
        debug!("start receive data");
        loop {
            let ticker = listener.recv().await;

            if let Ok(t) = ticker {
                dashboard_read.set_value(t.symbol.clone(), t).await;
            }
        }
    });

    let dashboard_read = dashboard.clone();
    tokio::spawn(async move {

        loop {
            info!("=====================================");
            let tickers = dashboard_read.get_all_entries();
            for t in &tickers {
                debug!("{:?}", t);
            }
            info!("total cache size: {}", tickers.len());
            sleep(Duration::from_millis(1000)).await;
        }
    });

    barrier.wait().await;
}

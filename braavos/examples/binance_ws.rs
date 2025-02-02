use braavos::binance::bn_models::BinanceBase;
use braavos::binance::bn_models::WsMethod::GetProperty;
use braavos::binance::bn_ws_commands::{connect_and_listen, WsRequest};
use braavos::cache::{DashBoard, RealTimeDashBoard};
use braavos::tools::setup_logger;
use log::LevelFilter;
use std::sync::Arc;
use tokio::sync::Barrier;

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

    tokio::spawn(async move {

        loop {
            let ticker = listener.recv().await;
            println!("Got ticker from {:?}", &ticker);
            let mut dashboard = RealTimeDashBoard::new();
            if let Ok(t) = ticker {
                dashboard.set_value(t.symbol.clone(), t).await;
            }
        }
    });

    barrier.wait().await;
}

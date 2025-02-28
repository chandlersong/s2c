use braavos::binance::bn_dashboard::get_spot_mini_ticker;
use braavos::tools::setup_logger;
use log::{info, LevelFilter};
use std::time::Duration;
use tokio::time::sleep;

///
/// 主要是对事实数据的监控
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
    let dashboard = get_spot_mini_ticker(1000).await;

    let dashboard_read = dashboard.clone();
    tokio::spawn(async move {
        loop {
            info!("============one loop started=============");
            let tickers = dashboard_read.get_all_entries();
            // for t in &tickers {
            //     debug!("{:?}", t);
            // }
            info!("total cache size: {}", tickers.len());
            sleep(Duration::from_millis(10 * 1000)).await;
        }
    });

    let mut minutes = 1;
    loop {
        sleep(Duration::from_secs(60)).await;
        info!("运行了{}分钟",minutes);
        minutes = minutes + 1;
    }
}

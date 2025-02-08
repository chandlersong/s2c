use crate::binance::bn_models::{BinanceBase, MiniTicker};
use crate::binance::bn_ws_commands::{connect_and_listen, BinanceWSClient};
use crate::cache::{DashBoard, FrequencyDashBoard};
use log::debug;
use tokio::sync::OnceCell;

///
/// 关于dashboard的我现在设想有两个思路。
/// 1. 对于每秒的ticker这类数据，其实订阅一次也就够了。所以用单利，然后clone一下
/// 2. 账户信息这种，websocket的连接则是需要一个一个建立。

static SPOT_WS_CLIENT: OnceCell<BinanceWSClient> = OnceCell::const_new();

static SPOT_TICKER_DASHBOARD: OnceCell<FrequencyDashBoard<MiniTicker>> = OnceCell::const_new();

async fn initial_spot_client() -> BinanceWSClient {
    let url = format!("{}/stream?streams=!miniTicker@arr", String::from(BinanceBase::WsSwapStreamUrl));
    connect_and_listen(url).await
}

pub async fn get_spot_client() -> BinanceWSClient {
    SPOT_WS_CLIENT.get_or_init(initial_spot_client).await.clone()
}

pub async fn get_spot_mini_ticker(frequency_mill_seconds: u64) -> FrequencyDashBoard<MiniTicker> {
    let res = SPOT_TICKER_DASHBOARD.get_or_init(|| async {
        let res = FrequencyDashBoard::new(frequency_mill_seconds).await;
        let mut dashboard_write = res.clone();
        tokio::spawn(async move {
            debug!("start receive data");
            loop {
                let ws_client = get_spot_client().await;
                let mut listener = ws_client.mini_ticker_tx.subscribe();
                loop {
                    let ticker = listener.recv().await;

                    if let Ok(t) = ticker {
                        dashboard_write.set_value(t.symbol.clone(), t).await;
                    }
                }
            }
        });
        res
    }).await;
    res.clone()
}
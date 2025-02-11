use crate::accounts::AccountReader;
use crate::binance::bn_models::WsMethod::SUBSCRIBE;
use crate::binance::bn_models::WsSubscribe::AllMiniTicker;
use crate::binance::bn_models::{BinanceBase, MiniTicker};
use crate::binance::bn_restful_commands::PMAccountReader;
use crate::binance::bn_ws_commands::{connect_and_listen, BinanceWSClient, WsRequest};
use crate::cache::{AutoUpdateValue, DashBoard, FrequencyDashBoard};
use crate::models::AccountSummary;
use crate::settings::Account;
use async_trait::async_trait;
use log::{debug, error};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OnceCell, RwLock};
use tokio::time::sleep;

///
/// 关于dashboard的我现在设想有两个思路。
/// 1. 对于每秒的ticker这类数据，其实订阅一次也就够了。所以用单利，然后clone一下
/// 2. 账户信息这种，websocket的连接则是需要一个一个建立。

static SPOT_WS_CLIENT: OnceCell<BinanceWSClient> = OnceCell::const_new();

static SPOT_TICKER_DASHBOARD: OnceCell<FrequencyDashBoard<MiniTicker>> = OnceCell::const_new();

async fn initial_spot_client() -> BinanceWSClient {
    let url = format!("{}/ws/spot", String::from(BinanceBase::WsSwapStreamUrl));
    connect_and_listen(url).await
}

pub async fn get_spot_client() -> BinanceWSClient {
    SPOT_WS_CLIENT.get_or_init(initial_spot_client).await.clone()
}

///
/// TODO: 初始化所有的symbol数据
pub async fn get_spot_mini_ticker(frequency_mill_seconds: u64) -> FrequencyDashBoard<MiniTicker> {
    let res = SPOT_TICKER_DASHBOARD.get_or_init(|| async {
        let res = FrequencyDashBoard::new(frequency_mill_seconds).await;
        let mut dashboard_write = res.clone();
        tokio::spawn(async move {
            debug!("start receive data");
            loop {
                let mut ws_client = get_spot_client().await;
                let params:Option<Vec<String>> = Some(vec![String::from(AllMiniTicker)]);
                let subscribe_request = WsRequest::new(SUBSCRIBE, params);
                ws_client.send_command(subscribe_request).await.expect("subscribe spot mini ticker failed");
                
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

#[derive(Clone)]
pub struct AccountDashBoard {
    value: Arc<RwLock<Option<AccountSummary>>>,
}


impl AccountDashBoard {
    pub async fn new(account: &Account, frequency_mill_seconds: u64) -> Self {
        let calculator = PMAccountReader::new(account.clone());
        let result = calculator.account_balance();


        let arc_value = match result {
            Ok(balance) => {
                Arc::new(RwLock::new(Some(balance)))
            }
            Err(err) => {
                error!("{}", err);
                Arc::new(RwLock::new(None))
            }
        };

        let update_value = arc_value.clone();

        tokio::spawn(
            async move {
                sleep(Duration::from_millis(frequency_mill_seconds)).await;
                loop {
                    let balance = calculator.account_balance();
                    match balance {
                        Ok(b) => {
                            update_value.write().await.replace(b);
                        }
                        Err(err) => {
                            error!("{}", err);
                        }
                    };

                    sleep(Duration::from_millis(frequency_mill_seconds)).await;
                }
            }
        );


        AccountDashBoard {
            value: arc_value.clone(),
        }
    }
}

#[async_trait]
impl AutoUpdateValue<AccountSummary> for AccountDashBoard {
    async fn get_value(&self) -> Result<AccountSummary, String> {
        let value = self.value.write().await.clone();
        match value {
            None => {
                Err(String::from("Account Dashboard is empty"))
            }
            Some(v) => {
                Ok(v)
            }
        }
    }
}
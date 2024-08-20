use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, PMBalance, SecurityInfo, Ticker};
use crate::clients::{AccountBalanceSummary, AccountValue};
use crate::errors::NightWatchError;
use crate::models::EmptyObject;
use crate::settings::SETTING;
use crate::utils::sign_hmac;
use lazy_static::lazy_static;
use log::{error, trace};
use rust_decimal_macros::dec;
use serde::de::DeserializeOwned;
use serde_json::Error as JsonError;
use std::fmt::Display;
use std::marker::PhantomData;
use url::Url;

lazy_static! {
    static ref CLIENT:  reqwest::Client = init_client();
}


impl CommandInfo<'_> {
    pub fn new(base: BinanceBase, path: BinancePath) -> CommandInfo<'static> {
        CommandInfo {
            base,
            path,
            security: None,
            client: &CLIENT,
        }
    }

    pub fn new_with_security(base: BinanceBase, path: BinancePath, api_key: &str, api_security: &str) -> CommandInfo<'static> {
        CommandInfo {
            base,
            path,
            security: Some(SecurityInfo {
                api_key: String::from(api_key),
                api_secret: String::from(api_security),
            }),
            client: &CLIENT,
        }
    }
}
pub(crate) async fn execute_ping() -> Result<(), NightWatchError> {
    let info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::PingAPI));

    let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
    let _ = get.execute(info, None).await?;
    Ok(())
}


fn init_client() -> reqwest::Client {
    let builder = reqwest::Client::builder();
    let proxy_builder = match &SETTING.proxy {
        Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
        None => { builder }
    };

    proxy_builder.build().unwrap()
}

pub trait BNCommand<T: Display, U: DeserializeOwned> {
    async fn execute(&self, info: CommandInfo, data: Option<T>) -> Result<U, NightWatchError>;
}


pub struct GetCommand<T: Display, U: DeserializeOwned> {
    pub(crate) phantom: PhantomData<(T, U)>,
}

impl<T: Display, U: DeserializeOwned> BNCommand<T, U> for GetCommand<T, U> {
    async fn execute(&self, info: CommandInfo<'_>, data: Option<T>) -> Result<U, NightWatchError> {
        let mut url = Url::parse(&String::from(info.base)).expect("Invalid base URL");
        url.set_path(&String::from(&String::from(info.path)));

        data.map(|request| {
            let query_param = format!("{}", request);

            let real_param = match &info.security {
                None => { query_param }
                Some(security) => {
                    let signature = sign_hmac(&query_param, &security.api_secret).unwrap();
                    format!("{query_param}&signature={signature}")
                }
            };

            url.set_query(Some(&real_param));
        });
        let request = match &info.security {
            None => { info.client.get(url) }
            Some(security) => {
                info.client.get(url).header(
                    "X-MBX-APIKEY", &security.api_key,
                )
            }
        };
        let res = request.send().await?;
        trace!("Response: {:?} {}", res.version(), res.status());
        trace!("Headers: {:#?}\n", res.headers());
        let body = res.text().await?;
        trace!("body:{}",&body);
        let result: Result<U, JsonError> = serde_json::from_str(&body);
        match result {
            Ok(resp1) => Ok(resp1),
            Err(_) => {
                error!("binance error response,{}",&body);
                panic!("binance request error!,response:{}", &body)
            }
        }

    }
}


pub struct PMAccountCalculator {}

impl AccountValue<PMBalance, Ticker> for PMAccountCalculator {
    fn account_value(&self, balance: &Vec<PMBalance>, ticker: &Vec<Ticker>) -> Result<AccountBalanceSummary, NightWatchError> {
        let mut um_value = dec!(0);
        let mut cm_value = dec!(0);
        let mut um_pnl = dec!(0);
        let mut cm_pnl = dec!(0);
        let mut cross_margin_borrow = dec!(0); //杠杆账户
        let mut acc_balance = dec!(0); //cross_margin_free
        let mut negative_balance = dec!(0);
        let mut usdt_balance = dec!(0);
        let mut usdt_spot_value = dec!(0);
        //TODO 加入资金套利的配置选项
        let mut funding_rates_arbitrage_pnl = dec!(0);
        for b in balance {
            um_pnl = um_pnl + b.um_unrealized_pnl;
            cm_pnl = cm_pnl + b.cm_unrealized_pnl;
            if b.asset == "USDT" {
                //swap如果有负债的话，USDT就不计算了。
                let mut swap_usdt = if b.cm_wallet_balance > dec!(0) { b.cm_wallet_balance } else { dec!(0) };
                swap_usdt = if b.um_wallet_balance > dec!(0) { swap_usdt + b.um_wallet_balance } else { swap_usdt };
                usdt_spot_value = b.cross_margin_free;
                usdt_balance = usdt_spot_value + swap_usdt;
                negative_balance = negative_balance + b.negative_balance;
            } else {
                let pair = format!("{}USDT", b.asset);
                if let Some(price) = ticker.iter().find(|t| t.symbol == pair) {
                    um_value = um_value + b.um_wallet_balance * price.price;
                    cm_value = cm_value + b.cm_wallet_balance * price.price;
                    cross_margin_borrow = cross_margin_borrow + b.cross_margin_borrowed * price.price;
                    acc_balance = acc_balance + b.cross_margin_free * price.price;
                    negative_balance = negative_balance + b.negative_balance * price.price;
                } else {
                    error!("symbol {} not exists!!!",b.asset)
                }
            }
        }

        let spot_equity = acc_balance + usdt_spot_value;
        let account_pnl = um_pnl + cm_pnl;
        Ok(AccountBalanceSummary {
            usdt_balance,
            negative_balance,
            account_pnl,
            spot_equity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::binance::CommandInfo;
    use crate::clients::binance_models::{BinanceBase, BinancePath, NormalAPI, PMBalance, PmAPI, TimeStampRequest, UMSwapPosition};
    use crate::models::EmptyObject;
    use crate::utils::{parse_test_json, setup_logger};
    use log::LevelFilter;

    #[test]
    fn test_account_value() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let calculator = PMAccountCalculator {};
        let actual = calculator.account_value(&balance, &ticker).unwrap();
        println!("{:?}", actual);
        assert_eq!(dec!(107.15440471), actual.usdt_balance);
        assert_eq!(dec!(1195.9985452450000000), actual.spot_equity);
        assert_eq!(dec!(-406.38234549), actual.negative_balance);
        assert_eq!(dec!(328.75345911), actual.account_pnl);
    }

    /** `test_account_value_with_um_cm_value` 测试计算account价值的单元测试
            # 测试内容
            1. um usdt和cm usdt都有值的时候，会加上去
    */
    #[test]
    fn test_account_value_with_um_cm_value() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance_v1.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let calculator = PMAccountCalculator {};
        let actual = calculator.account_value(&balance, &ticker).unwrap();
        println!("{:?}", actual);
        assert_eq!(dec!(109.15440471), actual.usdt_balance)
    }



    /**
    因为这里的方法，都是一些直接连接服务器的。所以都ignore了。需要去连接后面。
    **/

    #[ignore]
    #[tokio::test]
    async fn test_ping() {
        let info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::PingAPI));

        let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
        let x = get.execute(info, None).await.unwrap();
        assert_eq!(x, EmptyObject {})
    }


    #[ignore]
    #[tokio::test]
    async fn test_pm_swap_position() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let setting = &SETTING;
        let account = setting.get_account(0);

        let info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                  BinancePath::PAPI(PmAPI::SwapPositionAPI),
                                                  &account.api_key,
                                                  &account.secret);

        let get = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };
        let positions = get.execute(info, Some(Default::default())).await.unwrap();
        for p in &positions {
            println!("symbol:{},持仓未实现盈亏:{},名义价值:{}", p.symbol, p.unrealized_profit, p.notional);
        }
    }

    #[ignore]
    #[tokio::test]
    async fn test_pm_balance() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let setting = &SETTING;
        let account = setting.get_account(0);

        let info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                  BinancePath::PAPI(PmAPI::BalanceAPI),
                                                  &account.api_key,
                                                  &account.secret);

        let get = GetCommand::<TimeStampRequest, Vec<PMBalance>> { phantom: Default::default() };
        let positions = get.execute(info, Some(Default::default())).await.unwrap();
        for p in &positions {
            println!("symbol:{}", p.asset);
        }
    }
}

use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, PMBalance, SecurityInfo, Ticker, UMSwapPosition};
use crate::clients::{AccountBalanceSummary, AccountValue};
use crate::errors::NightWatchError;
use crate::models::{Decimal, EmptyObject};
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


pub struct PMAccountCalculator {
    fra_symbols: Vec<String>,
    burning_bnb: bool,
}

impl AccountValue<PMBalance, Ticker, UMSwapPosition> for PMAccountCalculator {
    fn account_value(&self, balance: &Vec<PMBalance>, ticker: &Vec<Ticker>, swap_position: &Vec<UMSwapPosition>) -> Result<AccountBalanceSummary, NightWatchError> {
        let mut um_pnl = dec!(0);
        let mut cm_pnl = dec!(0);
        let mut acc_balance = dec!(0); //cross_margin_free
        let mut negative_balance = dec!(0);
        let mut usdt_balance = dec!(0);
        let mut usdt_spot_value = dec!(0);
        //TODO 加入资金套利的配置选项
        let mut fra_position = dec!(0);
        for b in balance {
            um_pnl = um_pnl + b.um_unrealized_pnl;
            cm_pnl = cm_pnl + b.cm_unrealized_pnl;

            match b.asset.as_str() {
                "USDT" => {  //swap如果有负债的话，USDT就不计算了。
                    let mut swap_usdt = if b.cm_wallet_balance > dec!(0) { b.cm_wallet_balance } else { dec!(0) };
                    swap_usdt = if b.um_wallet_balance > dec!(0) { swap_usdt + b.um_wallet_balance } else { swap_usdt };
                    usdt_spot_value = b.cross_margin_free;
                    usdt_balance = usdt_spot_value + swap_usdt;
                    negative_balance = negative_balance + b.negative_balance;
                }
                "BNB" => {
                    if !self.burning_bnb {
                        (acc_balance, negative_balance) = cal_val(b, &acc_balance, &negative_balance, &ticker);
                    }
                }
                word if self.fra_symbols.contains(&String::from(word)) => {
                    let trading_pair = format!("{}USDT", b.asset);
                    if let (Some(price), Some(fra_swap)) = (ticker.iter().find(|t| t.symbol == trading_pair),
                                                            swap_position.iter().find(|t| t.symbol == trading_pair)) {
                        trace!("费率套利资金{},现货仓位{},现货价格{},永续利润{}",b.asset,b.cross_margin_free,price.price,fra_swap.unrealized_profit);
                        fra_position = fra_position + b.cross_margin_free * price.price + fra_swap.unrealized_profit;
                        um_pnl = um_pnl - fra_swap.unrealized_profit;
                    } else {
                        error!("{}没有找到对应值",trading_pair)
                    }
                }
                _ => {
                    (acc_balance, negative_balance) = cal_val(b, &acc_balance, &negative_balance, &ticker);
                }
            }
        }

        let spot_equity = acc_balance + usdt_spot_value;
        let account_pnl = um_pnl + cm_pnl;
        let account_equity = spot_equity + fra_position;
        Ok(AccountBalanceSummary {
            usdt_balance,
            negative_balance,
            account_pnl,
            account_equity,
        })
    }
}

fn cal_val(balance: &PMBalance, acc_balance: &Decimal, negative_balance: &Decimal, ticker: &Vec<Ticker>) -> (Decimal, Decimal) {
    let pair = format!("{}USDT", balance.asset);
    if let Some(price) = ticker.iter().find(|t| t.symbol == pair) {
        let um_symbol_value = &balance.um_wallet_balance * price.price;
        let cm_symbol_value = &balance.cm_wallet_balance * price.price;
        let value = balance.cross_margin_free * price.price;
        if value < dec!(5) {
            return (acc_balance.clone(), negative_balance.clone())
        };
        let acc_balance_res = acc_balance + value + cm_symbol_value + um_symbol_value;
        let negative_balance_res = negative_balance + balance.negative_balance * price.price;
        (acc_balance_res, negative_balance_res)
    } else {
        error!("symbol {} not exists!!!",balance.asset);
        (acc_balance.clone(), negative_balance.clone())
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
        let swap_position: Vec<UMSwapPosition> = parse_test_json::<Vec<UMSwapPosition>>("tests/data/binance_papi_um_position_risk.json");
        let calculator = PMAccountCalculator { fra_symbols: vec!["ETH".parse().unwrap(), "SOL".parse().unwrap()], burning_bnb: false };
        let actual = calculator.account_value(&balance, &ticker, &swap_position).unwrap();
        println!("{:?}", actual);
        assert_eq!(dec!(107.15440471), actual.usdt_balance);
        assert_eq!(dec!(1442.0434989420000000), actual.account_equity);
        assert_eq!(dec!(-406.38234549), actual.negative_balance);
        assert_eq!(dec!(88.05855911), actual.account_pnl);
    }

    #[test]
    fn test_account_value_burn_bnb() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let swap_position: Vec<UMSwapPosition> = parse_test_json::<Vec<UMSwapPosition>>("tests/data/binance_papi_um_position_risk.json");
        let calculator = PMAccountCalculator { fra_symbols: vec!["ETH".parse().unwrap(), "SOL".parse().unwrap()], burning_bnb: true };
        let actual = calculator.account_value(&balance, &ticker, &swap_position).unwrap();
        println!("{:?}", actual);
        assert_eq!(dec!(107.15440471), actual.usdt_balance);
        assert_eq!(dec!(1431.34435079000000), actual.account_equity);
        assert_eq!(dec!(-406.38234549), actual.negative_balance);
        assert_eq!(dec!(88.05855911), actual.account_pnl);
    }

    /** `test_account_value_with_um_cm_value` 测试计算account价值的单元测试
            # 测试内容
            1. um usdt和cm usdt都有值的时候，会加上去
                  2. 没有小于5u的过滤
                  3. 不存在币种不回影响最后结果
    */
    #[test]
    fn test_account_value_with_um_cm_value() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance_v1.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let swap_position: Vec<UMSwapPosition> = parse_test_json::<Vec<UMSwapPosition>>("tests/data/binance_papi_um_position_risk.json");
        let calculator = PMAccountCalculator { fra_symbols: vec![], burning_bnb: false };
        let actual = calculator.account_value(&balance, &ticker, &swap_position).unwrap();
        println!("{:?}", actual);
        assert_eq!(dec!(109.15440471), actual.usdt_balance);
        assert_eq!(dec!(1201.3485989420000000), actual.account_equity);
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

use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, PMBalance, SecurityInfo, Ticker, UMSwapPosition};
use crate::clients::{AccountBalanceSummary, AccountValue, SwapSummary};
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
    pub(crate) funding_rate_arbitrage: Vec<String>,
    pub(crate) burning_bnb: bool,
}

impl AccountValue<PMBalance, Ticker, UMSwapPosition> for PMAccountCalculator {
    fn account_balance(&self, balance: &Vec<PMBalance>, ticker: &Vec<Ticker>) -> AccountBalanceSummary {
        let mut swap_pnl = dec!(0);
        let mut total_balance = dec!(0); //cross_margin_free
        let mut negative_balance = dec!(0);
        let mut usdt_equity = dec!(0);
        for b in balance {
            swap_pnl = swap_pnl + b.um_unrealized_pnl + b.cm_unrealized_pnl;

            match b.asset.as_str() {
                "USDT" => {  //swap如果有负债的话，USDT就不计算了。
                    let mut swap_usdt = if b.cm_wallet_balance > dec!(0) { b.cm_wallet_balance } else { dec!(0) };
                    swap_usdt = if b.um_wallet_balance > dec!(0) { swap_usdt + b.um_wallet_balance } else { swap_usdt };
                    usdt_equity = b.cross_margin_free + swap_usdt;


                    total_balance = total_balance + b.total_wallet_balance;
                    negative_balance = negative_balance + b.negative_balance;
                }
                "BNB" => {
                    if !self.burning_bnb {
                        let (bal, pnl, negative) = cal_equity(b, ticker);
                        total_balance = total_balance + bal;
                        negative_balance = negative_balance + negative;
                        swap_pnl = swap_pnl + pnl
                    }
                }
                _ => {
                    let (bal, pnl, negative) = cal_equity(b, ticker);
                    total_balance = total_balance + bal;
                    negative_balance = negative_balance + negative;
                    swap_pnl = swap_pnl + pnl
                }
            }
        }

        let account_pnl = swap_pnl;
        let account_equity = total_balance + swap_pnl;
        AccountBalanceSummary {
            usdt_equity,
            negative_balance,
            account_pnl,
            account_equity,
        }
    }

    fn um_swap_balance(&self, swap_position: &Vec<UMSwapPosition>) -> SwapSummary {
        let fra_symbol: Vec<String> = self.funding_rate_arbitrage.iter().map(|x| format!("{}USDT", x)).collect();
        let mut balance = dec!(0);
        let mut short_balance = dec!(0);
        let mut long_balance = dec!(0);
        let mut pnl = dec!(0);
        let mut long_pnl = dec!(0);
        let mut short_pnl = dec!(0);
        let mut fra_pnl = dec!(0);

        for swap in swap_position {
            if fra_symbol.contains(&swap.symbol) {
                fra_pnl = fra_pnl + swap.unrealized_profit;
                continue;
            }
            trace!("symbol:{}, 名义价值：{},未实现利润{}", swap.symbol, swap.notional, swap.unrealized_profit);
            pnl = pnl + swap.unrealized_profit;
            if swap.position_amt > dec!(0) {
                balance = balance + swap.notional;
                long_balance = long_balance + swap.notional;
                long_pnl = long_pnl + swap.unrealized_profit;
            } else {
                let notional = swap.notional.abs();
                balance = balance + notional;
                short_balance = short_balance + notional;
                short_pnl = short_pnl + swap.unrealized_profit;
            }
        }
        SwapSummary {
            long_balance,
            long_pnl,
            short_balance,
            short_pnl,
            balance,
            pnl,
            fra_pnl,
        }
    }
}

/** cal_equity:通过balance和ticker计算几个。
* 返回的应该是total_balance,pnl和 negative_balance
*/
fn cal_equity(balance: &PMBalance, ticker: &Vec<Ticker>) -> (Decimal, Decimal, Decimal) {
    let pair = format!("{}USDT", balance.asset);
    if let Some(price) = ticker.iter().find(|t| t.symbol == pair) {
        let p = price.price;
        let spot_equity = balance.cross_margin_free * p;  //不能进行现货交易
        let total_balance;
        if spot_equity < dec!(5) {
            total_balance = dec!(0);
        } else {
            total_balance = balance.total_wallet_balance * p;
        };

        let negative_balance = balance.negative_balance * p;
        let swap_pnl = balance.um_unrealized_pnl * p + balance.cm_unrealized_pnl * p;
        trace!("{},total balance:{},pnl:{},negative balance{}",balance.asset,total_balance,swap_pnl,negative_balance);
        (total_balance, swap_pnl, negative_balance)
    } else {
        error!("symbol {} not exists!!!",balance.asset);
        (dec!(0), dec!(0), dec!(0))
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
    use tokio::join;

    #[test]
    fn test_um_swap_balance() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let swap_position: Vec<UMSwapPosition> = parse_test_json::<Vec<UMSwapPosition>>("tests/data/binance_papi_um_position_risk.json");
        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec![], burning_bnb: false };
        let actual = calculator.um_swap_balance(&swap_position);
        trace!("actual is {:?}",actual);
        assert_eq!(dec!(904.67784156), actual.long_balance);
        assert_eq!(dec!(26.86035050), actual.long_pnl);
        assert_eq!(dec!(2085.39024590), actual.short_balance);
        assert_eq!(dec!(254.90110885), actual.short_pnl);
        assert_eq!(dec!(2990.06808746), actual.balance);
        assert_eq!(dec!(281.76145935 ), actual.pnl)
    }

    #[test]
    fn test_um_swap_balance_with_fra() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let swap_position: Vec<UMSwapPosition> = parse_test_json::<Vec<UMSwapPosition>>("tests/data/binance_papi_um_position_risk.json");
        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec!["SOL".to_string(), "ETH".to_string()], burning_bnb: false };
        let actual = calculator.um_swap_balance(&swap_position);
        trace!("actual is {:?}",actual);
        assert_eq!(dec!(904.67784156), actual.long_balance);
        assert_eq!(dec!(26.86035050), actual.long_pnl);
        assert_eq!(dec!(922.03584590), actual.short_balance);
        assert_eq!(dec!(14.20620885), actual.short_pnl);
        assert_eq!(dec!(1826.71368746), actual.balance);
        assert_eq!(dec!(41.06655935), actual.pnl);
        assert_eq!(dec!(240.6949 ), actual.fra_pnl);
    }

    #[test]
    fn test_account_value() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec![], burning_bnb: false };
        let actual = calculator.account_balance(&balance, &ticker);
        println!("{:?}", actual);
        assert_eq!(dec!(107.15440471), actual.usdt_equity);
        assert_eq!(dec!(1016.5653078520000000), actual.account_equity);
        assert_eq!(dec!(-406.38234549), actual.negative_balance);
        assert_eq!(dec!(328.75345911), actual.account_pnl);
    }

    #[test]
    fn test_account_value_burn_bnb() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let balance: Vec<PMBalance> = parse_test_json::<Vec<PMBalance>>("tests/data/binance_papi_get_balance.json");
        let ticker: Vec<Ticker> = parse_test_json::<Vec<Ticker>>("tests/data/binance_spot_ticker.json");
        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec![], burning_bnb: true };
        let actual = calculator.account_balance(&balance, &ticker);
        println!("{:?}", actual);
        assert_eq!(dec!(107.15440471), actual.usdt_equity);
        assert_eq!(dec!(1005.86615970000000), actual.account_equity);
        assert_eq!(dec!(-406.38234549), actual.negative_balance);
        assert_eq!(dec!(328.75345911), actual.account_pnl);
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
        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec![], burning_bnb: false };
        let actual = calculator.account_balance(&balance, &ticker);
        println!("{:?}", actual);
        assert_eq!(dec!(109.15440471), actual.usdt_equity);
        assert_eq!(dec!(1016.5653078520000000), actual.account_equity);
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


    #[ignore]
    #[tokio::test]
    async fn test_real_pm_balance() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let setting = &SETTING;
        let account = setting.get_account(0);

        let pm_acc_balance_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                                 BinancePath::PAPI(PmAPI::BalanceAPI),
                                                                 &account.api_key,
                                                                 &account.secret);

        let acc_balance_command = GetCommand::<TimeStampRequest, Vec<PMBalance>> { phantom: Default::default() };


        let ticker_info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::SpotTickerAPI));

        let ticker_command = GetCommand::<EmptyObject, Vec<Ticker>> { phantom: Default::default() };


        let (acc_balance, ticker) = join!(
                acc_balance_command.execute(pm_acc_balance_info, Some(Default::default())),
                ticker_command.execute(ticker_info, None),
        );

        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec![], burning_bnb: false };
        let actual = calculator.account_balance(&acc_balance.unwrap()
                                                , &ticker.unwrap());
        println!("{:?}", actual)
    }

    #[ignore]
    #[tokio::test]
    async fn test_real_swap_balance() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let setting = &SETTING;
        let account = setting.get_account(0);

        let pm_acc_balance_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                                 BinancePath::PAPI(PmAPI::SwapPositionAPI),
                                                                 &account.api_key,
                                                                 &account.secret);

        let um_swap_position = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };


        let swap = um_swap_position.execute(pm_acc_balance_info, Some(Default::default())).await.unwrap();


        let calculator = PMAccountCalculator { funding_rate_arbitrage: vec!["SOL".to_string(), "ETH".to_string()], burning_bnb: false };
        let actual = calculator.um_swap_balance(&swap);
        println!("{:?}", actual)
    }
}

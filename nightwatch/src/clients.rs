use crate::clients::binance::{BNCommand, GetCommand, PMAccountCalculator};
use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, PMBalance, PmAPI, Ticker, TimeStampRequest, UMSwapPosition};
use crate::errors::NightWatchError;
use crate::models::{Decimal, EmptyObject};
use crate::settings::{Account, SETTING};
use log::error;
use prometheus::{Gauge, Opts};
use rust_decimal::prelude::ToPrimitive;
use tokio::join;

pub(crate) mod binance_models;
mod binance;


#[derive(Debug)]
pub struct AccountBalanceSummary {
    pub usdt_equity: Decimal,
    pub negative_balance: Decimal,
    pub account_pnl: Decimal,
    pub account_equity: Decimal,
}


#[derive(Debug)]
pub struct SwapSummary {
    pub long_balance: Decimal,
    pub long_pnl: Decimal,
    pub short_balance: Decimal,
    pub short_pnl: Decimal,
    pub balance: Decimal,
    pub pnl: Decimal,
}

pub trait AccountValue<T, U, X> {
    fn account_balance(&self, balance: &Vec<T>, ticker: &Vec<U>) -> Result<AccountBalanceSummary, NightWatchError>;

    fn um_swap_balance(&self, swap_position: &Vec<X>) -> Result<SwapSummary, NightWatchError>;
}

pub(crate) async fn ping_exchange() -> Result<(), NightWatchError> {
    binance::execute_ping().await.expect("can't connect to binance");
    println!("binance access success");
    Ok(())
}


pub(crate) async fn fetch_data() -> Result<Vec<Gauge>, NightWatchError> {
    let mut res = vec![];
    for acc in &SETTING.accounts {
        let positions_gauge = fetch_account_data(&acc).await;

        res.extend(positions_gauge)
    }
    Ok(res)
}

async fn fetch_account_data(acc: &Account) -> Vec<Gauge> {
    let swap_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                   BinancePath::PAPI(PmAPI::SwapPositionAPI),
                                                   &acc.api_key,
                                                   &acc.secret);

    let pm_acc_balance_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                             BinancePath::PAPI(PmAPI::BalanceAPI),
                                                             &acc.api_key,
                                                             &acc.secret);
    let ticker_info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::SpotTickerAPI));

    let acc_balance_command = GetCommand::<TimeStampRequest, Vec<PMBalance>> { phantom: Default::default() };
    let ticker_command = GetCommand::<EmptyObject, Vec<Ticker>> { phantom: Default::default() };
    let swap_position_command = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };


    let (acc_position, ticker, swap_position) = join!(
                acc_balance_command.execute(pm_acc_balance_info, Some(Default::default())),
                ticker_command.execute(ticker_info, None),
                swap_position_command.execute(swap_info,Some(Default::default()))

        );


    let fra = match &acc.funding_rate_arbitrage {
        None => { vec![] }
        Some(v) => { v.clone() }
    };
    let calculator = PMAccountCalculator {
        funding_rate_arbitrage: fra,
        burning_bnb: acc.burning_free,
    };


    let acc_balance = match (acc_position, ticker) {
        (Ok(acc_vec), Ok(ticker_vec)) => {
            Some(calculator.account_balance(&acc_vec, &ticker_vec).unwrap().to_prometheus(&acc.name))
        }
        (Err(e1), Err(e2)) => {
            error!("获得账户信息，e1:{},e2{}",e1,e2);
            None
        }
        _ => {
            error!("获得账户信息!");
            None
        }
    };

    let fra = match &acc.funding_rate_arbitrage {
        None => { vec![] }
        Some(v) => { v.iter().map(|x| format!("{}USDT", x)).collect() }
    };

    let mut swap_position_gauge: Vec<Gauge> = match swap_position {
        Ok(swap_position_vec) => {
            let mut res: Vec<Gauge> = swap_position_vec.iter()
                .filter(|s| !fra.contains(&s.symbol))
                .flat_map(|x| x.to_prometheus(&acc.name)).collect();

            match calculator.um_swap_balance(&swap_position_vec) {
                Ok(v) => {
                    res.extend(v.to_prometheus(&acc.name));
                    res
                }
                Err(_) => {
                    error!("整合swap account时出错");
                    res
                }
            }
        }
        Err(e) => {
            error!("获得账户信息，{}",e);
            vec![]
        }
    };

    match acc_balance {
        None => { swap_position_gauge }
        Some(v) => {
            swap_position_gauge.extend(v);
            swap_position_gauge
        }
    }
}

impl AccountBalanceSummary {
    pub fn to_prometheus(&self, strategy: &str) -> Vec<Gauge> {
        let side_name = format!("{}_acc_detail", strategy);
        let acc_equity = Gauge::with_opts(Opts::new(&side_name, format!("{0}_acc_help", strategy))
            .const_label("field", "equity")).unwrap();
        acc_equity.set(self.account_equity.to_f64().unwrap());

        let negative_balance = Gauge::with_opts(Opts::new(&side_name, format!("{0}_acc_help", strategy))
            .const_label("field", "negative_balance")).unwrap();
        negative_balance.set(self.negative_balance.to_f64().unwrap());

        let usdt_equity = Gauge::with_opts(Opts::new(&side_name, format!("{0}_acc_help", strategy))
            .const_label("field", "usdt_equity")).unwrap();
        usdt_equity.set(self.usdt_equity.to_f64().unwrap());

        let account_pnl = Gauge::with_opts(Opts::new(&side_name, format!("{0}_acc_help", strategy))
            .const_label("field", "account_pnl")).unwrap();
        account_pnl.set(self.account_pnl.to_f64().unwrap());

        vec![acc_equity, negative_balance, usdt_equity, account_pnl]
    }
}

impl SwapSummary {
    pub fn to_prometheus(&self, strategy: &str) -> Vec<Gauge> {
        let acc = Gauge::with_opts(Opts::new(&format!("{}_acc", strategy),
                                             format!("{0}_acc_help", strategy))).unwrap();
        acc.set(self.balance.to_f64().unwrap());

        let pnl = Gauge::with_opts(Opts::new(&format!("{}_acc_pnl", strategy),
                                             format!("{0}_acc_pnl_help", strategy))).unwrap();
        pnl.set(self.pnl.to_f64().unwrap());

        let acc_long = Gauge::with_opts(Opts::new(&format!("{}_acc_long", strategy),
                                                  format!("{0}_acc_long_help", strategy))).unwrap();
        acc_long.set(self.long_balance.to_f64().unwrap());

        let acc_long_pnl = Gauge::with_opts(Opts::new(&format!("{}_acc_long_pnl", strategy),
                                                      format!("{0}_acc_long_pnl_help", strategy))).unwrap();
        acc_long_pnl.set(self.long_pnl.to_f64().unwrap());

        let acc_short = Gauge::with_opts(Opts::new(&format!("{}_acc_short", strategy),
                                                   format!("{0}_acc_short_help", strategy))).unwrap();
        acc_short.set(self.short_balance.to_f64().unwrap());


        let acc_short_pnl = Gauge::with_opts(Opts::new(&format!("{}_acc_short_pnl", strategy),
                                                       format!("{0}_acc_short_pnl_help", strategy))).unwrap();
        acc_short_pnl.set(self.short_pnl.to_f64().unwrap());
        vec![acc, acc_long, acc_long_pnl, acc_short, acc_short_pnl, pnl]
    }
}

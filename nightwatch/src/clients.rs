use crate::clients::binance::BNCommand;
use crate::errors::NightWatchError;
use crate::models::{AccountSummary, SwapSummary};
use crate::prometheus_gauge;
use crate::prometheus_server::ToGauge;
use crate::settings::{Account, SETTING};
use prometheus::Gauge;

pub(crate) mod binance_models;
mod binance;


pub trait AccountReader<T, U, X> {
    fn account_balance(&self, account: &Account) -> AccountSummary;
}

pub(crate) async fn ping_exchange() -> Result<(), NightWatchError> {
    binance::execute_ping().await.expect("can't connect to binance");
    println!("binance access success");
    Ok(())
}


pub(crate) async fn cal_gauge_according_setting() -> Result<Vec<Gauge>, NightWatchError> {
    let mut res = vec![];
    for acc in &SETTING.accounts {
        let positions_gauge = cal_one_account_gauge(&acc).await;

        res.extend(positions_gauge)
    }
    Ok(res)
}

async fn cal_one_account_gauge(acc: &Account) -> Vec<Gauge> {
    // let swap_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
    //                                                BinancePath::PAPI(PmAPI::SwapPositionAPI),
    //                                                &acc.api_key,
    //                                                &acc.secret);
    //
    // let pm_acc_balance_info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
    //                                                          BinancePath::PAPI(PmAPI::BalanceAPI),
    //                                                          &acc.api_key,
    //                                                          &acc.secret);
    // let ticker_info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::SpotTickerAPI));
    //
    // let acc_balance_command = GetCommand::<TimeStampRequest, Vec<PMBalance>> { phantom: Default::default() };
    // let ticker_command = GetCommand::<EmptyObject, Vec<Ticker>> { phantom: Default::default() };
    // let swap_position_command = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };
    //
    //
    // let (acc_position, ticker, swap_position) = join!(
    //             acc_balance_command.execute(pm_acc_balance_info, Some(Default::default())),
    //             ticker_command.execute(ticker_info, None),
    //             swap_position_command.execute(swap_info,Some(Default::default()))
    //
    //     );
    //
    //
    // let fra = match &acc.funding_rate_arbitrage {
    //     None => { vec![] }
    //     Some(v) => { v.clone() }
    // };
    // let calculator = PMAccountReader {
    //     funding_rate_arbitrage: fra,
    //     burning_bnb: acc.burning_free,
    // };
    //
    //
    // let acc_balance = match (acc_position, ticker) {
    //     (Ok(acc_vec), Ok(ticker_vec)) => {
    //         Some(calculator.account_balance(&acc_vec, &ticker_vec).to_prometheus_gauge(&acc.name))
    //     }
    //     (Err(e1), Err(e2)) => {
    //         error!("获得账户信息，e1:{},e2{}",e1,e2);
    //         None
    //     }
    //     _ => {
    //         error!("获得账户信息!");
    //         None
    //     }
    // };
    //
    // let fra = match &acc.funding_rate_arbitrage {
    //     None => { vec![] }
    //     Some(v) => { v.iter().map(|x| format!("{}USDT", x)).collect() }
    // };
    //
    // let mut swap_position_gauge: Vec<Gauge> = match swap_position {
    //     Ok(swap_position_vec) => {
    //         let mut res: Vec<Gauge> = swap_position_vec.iter()
    //             .filter(|s| !fra.contains(&s.symbol))
    //             .flat_map(|x| x.to_prometheus_gauge(&acc.name)).collect();
    //
    //         let swap_gauge = calculator.um_swap_balance(&swap_position_vec).to_prometheus_gauge(&acc.name);
    //         res.extend(swap_gauge);
    //         res
    //     }
    //     Err(e) => {
    //         error!("获得账户信息，{}",e);
    //         vec![]
    //     }
    // };
    //
    // match acc_balance {
    //     None => { swap_position_gauge }
    //     Some(v) => {
    //         swap_position_gauge.extend(v);
    //         swap_position_gauge
    //     }
    // }
    vec![]
}

impl ToGauge for AccountSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side_name = format!("{}_acc_detail", strategy);
        let acc_equity = prometheus_gauge!(side_name,self.account_equity,("field" => "acc_equity"));
        let negative_balance = prometheus_gauge!(side_name,self.negative_balance,("field" => "negative_balance"));
        let usdt_equity = prometheus_gauge!(side_name,self.usdt_equity,("field" => "usdt_equity"));
        let account_pnl = prometheus_gauge!(side_name,self.account_pnl,("field" => "account_pnl"));
        vec![acc_equity, negative_balance, usdt_equity, account_pnl]
    }
}

impl ToGauge for SwapSummary {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let acc = prometheus_gauge!(format!("{}_acc", strategy),self.balance);
        let pnl = prometheus_gauge!(format!("{}_pnl", strategy),self.pnl);
        let acc_long = prometheus_gauge!(format!("{}_acc_long", strategy),self.long_balance);
        let acc_long_pnl = prometheus_gauge!(format!("{}_acc_long_pnl", strategy),self.long_pnl);
        let acc_short = prometheus_gauge!(format!("{}_acc_short", strategy),self.short_balance);
        let acc_short_pnl = prometheus_gauge!(format!("{}_acc_short_pnl", strategy),self.short_pnl);
        let fra_pnl = prometheus_gauge!(format!("{}_fra_pnl", strategy),self.fra_pnl);
        vec![acc, acc_long, acc_long_pnl, acc_short, acc_short_pnl, pnl, fra_pnl]
    }
}


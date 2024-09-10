use crate::errors::NightWatchError;

use crate::prometheus_gauge;
use crate::prometheus_server::ToGauge;
use braavos::accounts::{AccountReader, RawDataQuery};
use braavos::binance::binance_commands::{execute_ping, PMAccountReader, PMRawDataQuery};
use braavos::models::{AccountSummary, Decimal, SwapPosition, SwapSummary};
use braavos::settings::{Account, BRAAVOS_SETTING};
use prometheus::Gauge;
use rust_decimal_macros::dec;

pub(crate) async fn ping_exchange() -> Result<(), NightWatchError> {
    execute_ping().await.expect("can't connect to binance");
    println!("binance access success");
    Ok(())
}


pub(crate) async fn cal_gauge_according_setting() -> Result<Vec<Gauge>, NightWatchError> {
    let mut res = vec![];
    for acc in &BRAAVOS_SETTING.accounts {
        let positions_gauge = cal_one_account_gauge(&acc).await;

        res.extend(positions_gauge)
    }
    Ok(res)
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

impl ToGauge for SwapPosition {
    fn to_prometheus_gauge(&self, strategy: &str) -> Vec<Gauge> {
        let side = if self.position_amt > dec!(0) { dec!(1) } else { dec!(-1) };
        let side_name = if side == dec!(1) { format!("{strategy}_long") } else { format!("{strategy}_short") };

        let cur_price = prometheus_gauge!(side_name,self.cur_price,("field" => "cur_price"),("symbol" => &self.symbol));
        let avg_price = prometheus_gauge!(side_name,self.avg_price,("field" => "avg_price"),("symbol" => &self.symbol));
        let pos = prometheus_gauge!(side_name,self.position_amt,("field" => "pos"),("symbol" => &self.symbol));
        let pnl_u = prometheus_gauge!(side_name,self.pnl_u,("field" => "pnl_u"),("symbol" => &self.symbol));
        let value = prometheus_gauge!(side_name,self.pos_u,("field" => "value"),("symbol" => &self.symbol));


        let change_value: Decimal = (self.cur_price / self.avg_price - dec!(1)) * side;
        let change = prometheus_gauge!(side_name,change_value,("field" => "change"),("symbol" => &self.symbol));
        vec![cur_price, pos, pnl_u, avg_price, change, value]
    }
}


async fn cal_one_account_gauge(account: &Account) -> Vec<Gauge> {
    let query = PMRawDataQuery {};
    let raw_data = query.query_raw_data(account).await.unwrap();


    let funding_rate_arbitrage = match &account.funding_rate_arbitrage {
        None => { vec![] }
        Some(v) => { v.clone() }
    };

    let calculator = PMAccountReader { funding_rate_arbitrage, burning_bnb: account.burning_free };
    let data = calculator.account_balance(&raw_data);


    let mut res = vec![];
    res.extend(data.to_prometheus_gauge(&account.name));
    let um_swap = data.um_swap_summary;
    res.extend(um_swap.to_prometheus_gauge(&account.name));
    let um_swap_position = um_swap.positions;
    for p in &um_swap_position {
        res.extend(p.to_prometheus_gauge(&account.name));
    }
    res
}

#[cfg(test)]
mod tests {
    use crate::prometheus_server::ToGauge;
    use braavos::models::{AccountSummary, SwapPosition, SwapSummary};
    use rust_decimal_macros::dec;

    #[test]
    fn test_to_swap_position_prometheus() {
        let swap_position = SwapPosition {
            symbol: "bbb".to_string(),
            cur_price: dec!(1),
            avg_price: dec!(2),
            pos_u: Default::default(),
            pnl_u: Default::default(),
            position_amt: Default::default(),
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(6, actual.len());
    }

    #[test]
    fn test_to_swap_summary_prometheus() {
        let swap_position = SwapSummary {
            long_balance: Default::default(),
            long_pnl: Default::default(),
            short_balance: Default::default(),
            short_pnl: Default::default(),
            balance: Default::default(),
            pnl: Default::default(),
            fra_pnl: Default::default(),
            positions: vec![],
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(7, actual.len());
    }

    #[test]
    fn test_to_account_summary_prometheus() {
        let swap_position = AccountSummary {
            usdt_equity: Default::default(),
            negative_balance: Default::default(),
            account_pnl: Default::default(),
            account_equity: Default::default(),
            um_swap_summary: SwapSummary {
                long_balance: Default::default(),
                long_pnl: Default::default(),
                short_balance: Default::default(),
                short_pnl: Default::default(),
                balance: Default::default(),
                pnl: Default::default(),
                fra_pnl: Default::default(),
                positions: vec![],
            },
        };
        let actual = swap_position.to_prometheus_gauge("test");

        assert_eq!(4, actual.len());
    }
}




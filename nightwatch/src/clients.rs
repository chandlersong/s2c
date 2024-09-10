use crate::errors::NightWatchError;

use braavos::accounts::{AccountReader, RawDataQuery};
use braavos::binance::binance_commands::{execute_ping, PMAccountReader, PMRawDataQuery};
use braavos::settings::{Account, BRAAVOS_SETTING};
use prometheus::Gauge;

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
    // res.extend(data.to_prometheus_gauge(&account.name));
    let um_swap = data.um_swap_summary;
    // res.extend(um_swap.to_prometheus_gauge(&account.name));
    let um_swap_position = um_swap.positions;
    for p in &um_swap_position {
        // res.extend(p.to_prometheus_gauge(&account.name));
    }
    res
}




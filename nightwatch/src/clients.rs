use crate::clients::binance::{BNCommand, GetCommand};
use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, PmAPI, TimeStampRequest, UMSwapPosition};
use crate::errors::NightWatchError;
use crate::models::{AccountBalance, Decimal};
use crate::settings::SETTING;
use log::error;
use prometheus::Gauge;

pub(crate) mod binance_deprecated;
pub(crate) mod binance_models;
mod binance;


#[derive(Debug)]
pub struct AccountBalanceSummary {
    pub usdt_equity: Decimal,
    pub negative_balance: Decimal,
    pub account_pnl: Decimal,
    pub account_equity: Decimal,
}


pub trait AccountValue<T, U, X> {
    fn account_balance(&self, balance: &Vec<T>, ticker: &Vec<U>) -> Result<AccountBalanceSummary, NightWatchError>;
}

pub(crate) async fn ping_exchange() -> Result<(), NightWatchError> {
    binance::execute_ping().await.expect("can't connect to binance");
    println!("binance access success");
    Ok(())
}


pub(crate) async fn fetch_prices() -> Result<Vec<Gauge>, NightWatchError> {
    let mut res = vec![];
    for acc in &SETTING.accounts {
        let info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                  BinancePath::PAPI(PmAPI::SwapPositionAPI),
                                                  &acc.api_key,
                                                  &acc.secret);

        let get = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };
        let positions_gauge: Vec<Gauge> = match get.execute(info, Some(Default::default())).await {
            Ok(result) => result.iter().flat_map(|x| x.to_prometheus(&acc.name)).collect(),
            Err(error) => {
                error!("Error: {}", error);
                vec![]
            },
        };

        res.extend(positions_gauge)
    }
    Ok(res)
}

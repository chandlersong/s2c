use crate::clients::binance::{BNCommand, GetCommand};
use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, PmAPI, TimeStampRequest, UMSwapPosition};
use crate::errors::NightWatchError;
use crate::models::AccountBalance;
use crate::settings::SETTING;
use prometheus::Gauge;

pub(crate) mod binance_deprecated;
pub(crate) mod binance_models;
mod binance;


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
        let positions_gauge: Vec<Gauge> = get.execute(info, Some(Default::default())).await.unwrap()
            .iter().flat_map(|x| x.to_prometheus(&acc.name)).collect();
        res.extend(positions_gauge)
    }
    Ok(res)
}

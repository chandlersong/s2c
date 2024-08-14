use crate::errors::NightWatchError;
use crate::models::AccountBalance;

pub(crate) mod binance_deprecated;
pub(crate) mod binance_models;
mod binance;


pub(crate) async fn ping_exchange() -> Result<(), NightWatchError> {
    binance::execute_ping().await.expect("can't connect to binance");
    println!("binance access success");
    Ok(())
}

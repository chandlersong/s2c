use crate::errors::NightWatchError;
use crate::models::AccountBalance;

pub(crate) mod binance_deprecated;
pub(crate) mod binance_models;
mod binance;

trait Client {
    async fn ping(&self) -> Result<(), NightWatchError>;

    async fn account_balance(&self) -> Result<AccountBalance, NightWatchError>;
}




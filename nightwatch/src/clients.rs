use crate::errors::NightWatchError;
use crate::models::AccountBalance;

pub(crate) mod binance;
pub(crate) mod binance_models;

trait Client {
    async fn ping(&self) -> Result<(), NightWatchError>;

    async fn account_balance(&self) -> Result<AccountBalance, NightWatchError>;
}




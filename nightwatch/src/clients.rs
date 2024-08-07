use crate::errors::NightWatchError;

pub(crate) mod binance;
pub(crate) mod binance_models;


struct AccountBalance {}

trait Exchange {
    async fn ping(&self) -> Result<(), NightWatchError>;

    async fn account_balance(&self) -> Result<AccountBalance, NightWatchError>;
}




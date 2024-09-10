use crate::errors::BraavosError;
use crate::models::AccountSummary;
use crate::settings::Account;

/** 把一些原始的数据读出

*/
pub trait RawDataQuery<X> {
    async fn query_raw_data(&self, account: &Account) -> Result<X, BraavosError>;
}


pub trait AccountReader<X> {
    fn account_balance(&self, raw_data: &X) -> AccountSummary;
}

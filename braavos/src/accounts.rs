use crate::errors::BraavosError;
use crate::models::AccountSummary;
use crate::settings::Account;
use async_trait::async_trait;

/** 把一些原始的数据读出

*/
#[deprecated(since = "版本号", note = "不用这么做了")]
pub(crate) trait RawDataQuery<X> {
    async fn query_raw_data(&self, account: &Account) -> Result<X, BraavosError>;
}


#[deprecated(since = "版本号", note = "不用这么做了")]
#[async_trait]
pub trait AccountReader {
    async fn account_balance(&self) -> Result<AccountSummary, BraavosError>;
}

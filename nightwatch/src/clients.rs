pub(crate) mod binance;

trait Exchange {
    async fn ping(&self) -> Result<(), reqwest::Error> ;

    async fn get_account_info(&self) -> Result<(), reqwest::Error> ;
}

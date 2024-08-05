pub(crate) mod binance;

trait Exchange {
    async fn ping(&self) -> Result<(), reqwest::Error> ;
}

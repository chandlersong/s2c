use lazy_static::lazy_static;
use reqwest::Client;
use url::Url;

use crate::clients::Exchange;

lazy_static! {
    static ref BINANCE_URL: String = String::from("https://api.binance.com/");
}

lazy_static! {
    static ref PING_PATH: String = String::from("api/v3/ping");
}


struct BinanceExchange {
    account_name: String,
    client: Client,
    base_url: String,
}


impl Exchange for BinanceExchange {
    async fn ping(&self) -> Result<(), reqwest::Error> {
        let mut url = Url::parse(&self.base_url).expect("Invalid base URL");
        url.set_path(&PING_PATH);
        let res = self.client.get(url).send().await?;

        eprintln!("Response: {:?} {}", res.version(), res.status());
        eprintln!("Headers: {:#?}\n", res.headers());

        let body = res.text().await?;

        println!("{body}");

        Ok(())
    }
}

impl BinanceExchange {
    pub fn new(account_name: String, proxy_url: Option<String>, binance_url: Option<String>) -> BinanceExchange {
        let builder = reqwest::Client::builder();
        let proxy_builder = match proxy_url {
            Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
            None => { builder }
        };

        let client = proxy_builder.build().unwrap();
        let base_url = match binance_url {
            Some(val) => { val }
            None => BINANCE_URL.clone()
        };
        BinanceExchange {
            client,
            account_name,
            base_url,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_ping() {
        let proxy = Option::from(String::from("http://localhost:7890"));
        let base_url = Option::from(BINANCE_URL.clone());
        let exchange = BinanceExchange::new(String::from("test"), proxy, base_url);
        assert_eq!("test", exchange.account_name);
        exchange.ping().await.unwrap();
    }
}

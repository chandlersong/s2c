use lazy_static::lazy_static;
use hmac::{Hmac, Mac};
use sha2::{Sha256};
use sha2::digest::InvalidLength;

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

// 签名方法从官方项目copy https://github.com/binance/binance-spot-connector-rust/blob/main/src/utils.rs#L9
fn sign_hmac(payload: &str, key: &str) -> Result<String, InvalidLength> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

    mac.update(payload.to_string().as_bytes());
    let result = mac.finalize();
    Ok(format!("{:x}", result.into_bytes()))
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

    #[test]
    fn sign_payload_with_hmac_test() {
        let payload = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let key = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";

        let signature = super::sign_hmac(payload, key).unwrap();

        assert_eq!(
            signature,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71".to_owned()
        );
    }


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

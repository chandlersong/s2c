use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use reqwest::Client;
use rust_decimal_macros::dec;
use sha2::digest::InvalidLength;
use sha2::Sha256;
use url::Url;

use crate::clients::{AccountBalance, Exchange};
use crate::clients::binance_models::{PMBalance, UMSwapBalance};
use crate::errors::NightWatchError;
use crate::models::UnixTimeStamp;
use crate::settings::Account;

enum BinanceURL {
    Normal,
    PortfolioMargin,
}


impl From<BinanceURL> for String {
    fn from(url: BinanceURL) -> Self {
        String::from(
            match url {
                BinanceURL::Normal => String::from("https://api.binance.com/"),
                BinanceURL::PortfolioMargin => String::from("https://papi.binance.com/")
            }
        )
    }
}

enum API {
    Normal(NormalAPI),
    PAPI(PAPI),
}

enum NormalAPI {
    PING
}

enum PAPI {
    Balance,
    SwapPosition,
}

impl From<API> for String {
    fn from(api: API) -> Self {
        String::from(
            match api {
                API::Normal(route) => match route {
                    NormalAPI::PING => String::from("api/v3/ping"),
                }
                API::PAPI(route) => match route {
                    PAPI::Balance => String::from("/papi/v1/balance"),
                    PAPI::SwapPosition => String::from("/papi/v1/um/account"),
                }
            }
        )
    }
}


// 签名方法从官方项目copy https://github.com/binance/binance-spot-connector-rust/blob/main/src/utils.rs#L9
fn sign_hmac(payload: &str, key: &str) -> Result<String, InvalidLength> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;

    mac.update(payload.to_string().as_bytes());
    let result = mac.finalize();
    Ok(format!("{:x}", result.into_bytes()))
}


fn unix_time() -> UnixTimeStamp {
    let now = SystemTime::now();
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap();
    since_epoch.as_secs() * 1000 + u64::from(since_epoch.subsec_nanos()) / 1_000_000
}

struct BinancePMExchange {
    account: Account,
    client: Client,
    trade_url: String,
}


impl Exchange for BinancePMExchange {
    async fn ping(&self) -> Result<(), NightWatchError> {
        let mut url = Url::parse(&String::from(BinanceURL::Normal)).expect("Invalid base URL");
        url.set_path(&String::from(API::Normal(NormalAPI::PING)));
        let res = self.client.get(url).send().await?;

        eprintln!("Response: {:?} {}", res.version(), res.status());
        eprintln!("Headers: {:#?}\n", res.headers());

        let body = res.text().await?;

        println!("{body}");

        Ok(())
    }

    async fn account_balance(&self) -> Result<AccountBalance, NightWatchError> {
        let assets = self.get_balance().await?;
        for a in &assets{
            println!("{}", a.asset)
        }

        self.get_swap_position().await?;
        Ok(AccountBalance{})
    }
}

impl BinancePMExchange {
    pub fn new(setting_account: &Account, proxy_url: &Option<String>) -> BinancePMExchange {
        BinancePMExchange::new_with_url(setting_account, proxy_url, Some(BinanceURL::PortfolioMargin))
    }

    pub fn new_with_url(setting_account: &Account, proxy_url: &Option<String>, binance_url: Option<BinanceURL>) -> BinancePMExchange {
        let builder = reqwest::Client::builder();
        let proxy_builder = match proxy_url {
            Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
            None => { builder }
        };

        let client = proxy_builder.build().unwrap();
        let trade_url = match binance_url {
            Some(val) => String::from(val),
            None => String::from(BinanceURL::PortfolioMargin)
        };
        let account: Account = setting_account.clone();
        BinancePMExchange {
            client,
            account,
            trade_url,
        }
    }

     async fn get_balance(&self) -> Result<Vec<PMBalance>, NightWatchError> {
         let timestamp = unix_time();
         let query_param = format!("timestamp={timestamp}");
         let signature = sign_hmac(&query_param, &self.account.secret).unwrap();
         let real_param = format!("{query_param}&signature={signature}");

         let mut url = Url::parse(&self.trade_url).expect("Invalid base URL");
         url.set_path(&String::from(API::PAPI(PAPI::Balance)));
         url.set_query(Some(&real_param));
         let res = self.client.get(url).header("X-MBX-APIKEY", &self.account.api_key).send().await?;

         let assets = res.json().await?;
         Ok(assets)
     }

    async fn get_swap_position(&self) -> Result<UMSwapBalance, NightWatchError> {
        let timestamp = unix_time();
        let rec_window = 5000;
        let query_param = format!("timestamp={timestamp}&recvWindow={rec_window}");
        let signature = sign_hmac(&query_param, &self.account.secret).unwrap();
        let real_param = format!("{query_param}&signature={signature}");

        let mut url = Url::parse(&self.trade_url).expect("Invalid base URL");
        url.set_path(&String::from(API::PAPI(PAPI::SwapPosition)));
        url.set_query(Some(&real_param));
        let res = self.client.get(url).header("X-MBX-APIKEY", &self.account.api_key).send().await?;

        let mut balance: UMSwapBalance = res.json().await?;
        balance.positions.retain(|p| p.maint_margin != dec!(0));
        Ok(balance)
    }

}


#[cfg(test)]
mod tests {
    use crate::settings::Settings;

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
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMExchange::new(account, &setting.proxy);
        assert_eq!("cyc", exchange.account.name);
        exchange.ping().await.unwrap();
    }


    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_get_balance() {
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMExchange::new(account, &setting.proxy);

        let data = exchange.get_balance().await.unwrap();
        for b in &data {
            println!("{}", b.asset)
        }
    }

    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_get_swap_position() {
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMExchange::new(account, &setting.proxy);

        let balance = exchange.get_swap_position().await.unwrap();

        println!("group_id:{}", balance.trade_group_id);
        println!("assert assert size :{}", balance.assets.len());
        println!("assert position size :{}", balance.positions.len());

        for p in &balance.positions {
            assert_ne!(p.maint_margin, dec!(0))
        }
    }
}

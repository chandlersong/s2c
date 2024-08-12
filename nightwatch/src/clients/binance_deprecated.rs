use async_trait::async_trait;
use mockall::automock;
use url::Url;

use crate::clients::binance_models::{BinanceBase, BinancePath, Future, NormalAPI, PMBalance, PmAPI, Ticker, UMSwapPosition};
use crate::clients::{AccountBalance, Client};
use crate::errors::NightWatchError;
use crate::models::SwapBalance;
use crate::settings::Account;
use crate::utils::{sign_hmac, unix_time};


struct BinanceClient {
    api: Box<dyn BinanceAPI>,
}

impl BinanceClient {
    fn new(setting_account: &Account, proxy_url: &Option<String>) -> BinanceClient {
        BinanceClient {
            api: Box::new(BinancePMAPI::new(setting_account, proxy_url)),
        }
    }
}


impl Client for BinanceClient {
    async fn ping(&self) -> Result<(), NightWatchError> {
        // self.api.ping().await.expect("can't connect to binance");
        Ok(())
    }

    async fn account_balance(&self) -> Result<AccountBalance, NightWatchError> {
        let swap = self.api.get_swap_position().await?
            .iter().map(|o| SwapBalance::from(o)).collect();


        Ok(AccountBalance {
            swap,
        })
    }
}

#[cfg_attr(test, automock)]
#[async_trait]
trait BinanceAPI {
    async fn ping(&self) -> Result<(), NightWatchError>;
    async fn get_balance(&self) -> Result<Vec<PMBalance>, NightWatchError>;
    async fn get_swap_position(&self) -> Result<Vec<UMSwapPosition>, NightWatchError>;
    async fn swap_ticker(&self) -> Result<Vec<Ticker>, NightWatchError>;
    async fn spot_ticker(&self) -> Result<Vec<Ticker>, NightWatchError>;
}

struct BinancePMAPI {
    account: Account,
    client: reqwest::Client,
    trade_url: String,
}

impl BinancePMAPI {
    pub fn new(setting_account: &Account, proxy_url: &Option<String>) -> BinancePMAPI {
        BinancePMAPI::new_with_url(setting_account, proxy_url, Some(BinanceBase::PortfolioMargin))
    }

    pub fn new_with_url(setting_account: &Account, proxy_url: &Option<String>, binance_url: Option<BinanceBase>) -> BinancePMAPI {
        let builder = reqwest::Client::builder();
        let proxy_builder = match proxy_url {
            Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
            None => { builder }
        };

        let client = proxy_builder.build().unwrap();
        let trade_url = match binance_url {
            Some(val) => String::from(val),
            None => String::from(BinanceBase::PortfolioMargin)
        };
        let account: Account = setting_account.clone();
        BinancePMAPI {
            client,
            account,
            trade_url,
        }
    }
}

#[automock]
#[async_trait]
impl BinanceAPI for BinancePMAPI {
    async fn ping(&self) -> Result<(), NightWatchError> {
        let mut url = Url::parse(&String::from(BinanceBase::Normal)).expect("Invalid base URL");
        url.set_path(&String::from(BinancePath::Normal(NormalAPI::PingAPI)));
        let res = self.client.get(url).send().await?;

        eprintln!("Response: {:?} {}", res.version(), res.status());
        eprintln!("Headers: {:#?}\n", res.headers());

        let body = res.text().await?;

        println!("{body}");

        Ok(())
    }

    async fn get_balance(&self) -> Result<Vec<PMBalance>, NightWatchError> {
        let timestamp = unix_time();
        let query_param = format!("timestamp={timestamp}");
        let signature = sign_hmac(&query_param, &self.account.secret).unwrap();
        let real_param = format!("{query_param}&signature={signature}");

        let mut url = Url::parse(&self.trade_url).expect("Invalid base URL");
        url.set_path(&String::from(BinancePath::PAPI(PmAPI::BalanceAPI)));
        url.set_path(&String::from(BinancePath::PAPI(PmAPI::BalanceAPI)));
        url.set_query(Some(&real_param));
        let res = self.client.get(url).header("X-MBX-APIKEY", &self.account.api_key).send().await?;

        let assets = res.json().await?;
        Ok(assets)
    }

    async fn get_swap_position(&self) -> Result<Vec<UMSwapPosition>, NightWatchError> {
        let timestamp = unix_time();
        let rec_window = 5000;
        let query_param = format!("timestamp={timestamp}&recvWindow={rec_window}");
        let signature = sign_hmac(&query_param, &self.account.secret).unwrap();
        let real_param = format!("{query_param}&signature={signature}");

        let mut url = Url::parse(&self.trade_url).expect("Invalid base URL");
        url.set_path(&String::from(BinancePath::PAPI(PmAPI::SwapPositionAPI)));
        url.set_query(Some(&real_param));
        let res = self.client.get(url).header("X-MBX-APIKEY", &self.account.api_key).send().await?;

        let balance: Vec<UMSwapPosition> = res.json().await.expect("result is not right");
        Ok(balance)
    }

    async fn swap_ticker(&self) -> Result<Vec<Ticker>, NightWatchError> {
        let mut url = Url::parse(&String::from(BinanceBase::SWAP)).expect("Invalid base URL");
        url.set_path(&String::from(BinancePath::FAPI(Future::SwapTickerAPI)));
        let res = self.client.get(url).send().await?;
        let ticker: Vec<Ticker> = res.json().await?;
        Ok(ticker)
    }

    async fn spot_ticker(&self) -> Result<Vec<Ticker>, NightWatchError> {
        let mut url = Url::parse(&String::from(BinanceBase::Normal)).expect("Invalid base URL");
        url.set_path(&String::from(BinancePath::Normal(NormalAPI::SpotTickerAPI)));
        let res = self.client.get(url).send().await?;
        let ticker: Vec<Ticker> = res.json().await?;
        Ok(ticker)
    }
}


#[cfg(test)]
mod tests {
    use crate::models::Decimal;
    use crate::settings::Settings;
    use rust_decimal_macros::dec;

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
        let exchange = BinancePMAPI::new(account, &setting.proxy);
        assert_eq!("cyc", exchange.account.name);
        exchange.ping().await.unwrap();
    }


    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_get_balance() {
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMAPI::new(account, &setting.proxy);

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
        let exchange = BinancePMAPI::new(account, &setting.proxy);

        let positions = exchange.get_swap_position().await.unwrap();


        for p in &positions {
            println!("symbol:{},持仓未实现盈亏:{}", p.symbol, p.unrealized_profit);
        }
    }


    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_swap_ticker() {
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMAPI::new(account, &setting.proxy);

        let tickers = exchange.swap_ticker().await.unwrap();

        println!("ticker size:{}", tickers.len());
    }

    #[tokio::main]
    #[ignore]
    #[test]
    async fn test_spot_ticker() {
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);
        let exchange = BinancePMAPI::new(account, &setting.proxy);

        let tickers = exchange.spot_ticker().await.unwrap();

        for t in &tickers {
            println!("{}", t.symbol)
        }
        // println!("ticker size:{}", tickers.len());
    }

    #[tokio::test]
    async fn test_account_balance() {
        let mut mock_api = MockBinanceAPI::new();


        let positions = vec![
            random_um_swap_position("sample", dec!(10), dec!(2), dec!(1), dec!(2.1))
        ];

        mock_api.expect_get_swap_position()
            .times(1)
            .returning(move || Ok(positions.clone()));

        let client = BinanceClient {
            api: Box::new(mock_api)
        };
        let account = client.account_balance().await.unwrap();
        let swap = &account.swap;
        let has_sample_symbol = swap.iter().any(|coin|
            coin.symbol == "sample" && coin.position == dec!(10)
                && coin.cost_price == dec!(2) && coin.unrealized_profit == dec!(1)
                && coin.price == dec!(2.1));
        assert_eq!(swap.len(), 1, "生成swap的数组不对");
        assert!(has_sample_symbol, "不存在测试币");
    }


    fn random_um_swap_position(symbol: &str, position_amt: Decimal, entry_price: Decimal, unrealized_profit: Decimal, mark_price: Decimal) -> UMSwapPosition {
        UMSwapPosition {
            entry_price,
            leverage: 0,
            mark_price,
            max_notional_value: Default::default(),
            position_amt,
            notional: Default::default(),
            symbol: symbol.to_string(),
            unrealized_profit,
            liquidation_price: Default::default(),
            position_side: "BOTH".to_string(),
            update_time: 0,
            break_even_price: Default::default(),
        }
    }
}

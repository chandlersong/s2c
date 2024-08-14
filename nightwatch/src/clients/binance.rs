use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI, SecurityInfo};
use crate::errors::NightWatchError;
use crate::models::EmptyObject;
use crate::settings::Settings;
use crate::utils::sign_hmac;
use lazy_static::lazy_static;
use log::{error, trace};
use serde::de::DeserializeOwned;
use serde_json::Error as JsonError;
use std::env;
use std::fmt::Display;
use std::marker::PhantomData;
use url::Url;

lazy_static! {
    static ref CLIENT:  reqwest::Client = init_client();
}


impl CommandInfo<'_> {
    pub fn new(base: BinanceBase, path: BinancePath) -> CommandInfo<'static> {
        CommandInfo {
            base,
            path,
            security: None,
            client: &CLIENT,
        }
    }

    pub fn new_with_security(base: BinanceBase, path: BinancePath, api_key: &str, api_security: &str) -> CommandInfo<'static> {
        CommandInfo {
            base,
            path,
            security: Some(SecurityInfo {
                api_key: String::from(api_key),
                api_secret: String::from(api_security),
            }),
            client: &CLIENT,
        }
    }
}
pub(crate) async fn execute_ping() -> Result<(), NightWatchError> {
    let info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::PingAPI));

    let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
    let _ = get.execute(info, None).await?;
    Ok(())
}


fn init_client() -> reqwest::Client {
    let config_path = env::var("NIGHT_WATCH_CONFIG").unwrap_or_else(|_| String::from("conf/Settings.toml"));
    let setting = Settings::new(&config_path).unwrap();
    let builder = reqwest::Client::builder();
    let proxy_builder = match setting.proxy {
        Some(val) => { builder.proxy(reqwest::Proxy::https(val).unwrap()) }
        None => { builder }
    };

    proxy_builder.build().unwrap()
}

trait BNCommand<T: Display, U: DeserializeOwned> {
    async fn execute(&self, info: CommandInfo, data: Option<T>) -> Result<U, NightWatchError>;
}


pub struct GetCommand<T: Display, U: DeserializeOwned> {
    phantom: PhantomData<(T, U)>,
}

impl<T: Display, U: DeserializeOwned> BNCommand<T, U> for GetCommand<T, U> {
    async fn execute(&self, info: CommandInfo<'_>, data: Option<T>) -> Result<U, NightWatchError> {
        let mut url = Url::parse(&String::from(info.base)).expect("Invalid base URL");
        url.set_path(&String::from(&String::from(info.path)));

        data.map(|request| {
            let query_param = format!("{}", request);

            let real_param = match &info.security {
                None => { query_param }
                Some(security) => {
                    let signature = sign_hmac(&query_param, &security.api_secret).unwrap();
                    format!("{query_param}&signature={signature}")
                }
            };

            url.set_query(Some(&real_param));
        });
        let request = match &info.security {
            None => { info.client.get(url) }
            Some(security) => {
                info.client.get(url).header(
                    "X-MBX-APIKEY", &security.api_key,
                )
            }
        };
        let res = request.send().await?;
        trace!("Response: {:?} {}", res.version(), res.status());
        trace!("Headers: {:#?}\n", res.headers());
        let body = res.text().await?;
        trace!("body:{}",&body);
        let result: Result<U, JsonError> = serde_json::from_str(&body);
        match result {
            Ok(resp1) => Ok(resp1),
            Err(_) => {
                error!("binance error response,{}",&body);
                panic!("binance request error!,response:{}", &body)
            }
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::binance::CommandInfo;
    use crate::clients::binance_models::{BinanceBase, BinancePath, NormalAPI, PmAPI, TimeStampRequest, UMSwapPosition};
    use crate::models::EmptyObject;
    use crate::utils::setup_logger;
    use log::LevelFilter;

    /**
    因为这里的方法，都是一些直接连接服务器的。所以都ignore了。需要去连接后面。
    **/

    #[ignore]
    #[tokio::test]
    async fn test_ping() {
        let info = CommandInfo::new(BinanceBase::Normal, BinancePath::Normal(NormalAPI::PingAPI));

        let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
        let x = get.execute(info, None).await.unwrap();
        assert_eq!(x, EmptyObject {})
    }


    #[ignore]
    #[tokio::test]
    async fn test_pm_swap_position() {
        let _ = setup_logger(Some(LevelFilter::Trace));
        let setting = Settings::new("conf/Settings.toml").unwrap();
        let account = setting.get_account(0);

        let info = CommandInfo::new_with_security(BinanceBase::PortfolioMargin,
                                                  BinancePath::PAPI(PmAPI::SwapPositionAPI),
                                                  &account.api_key,
                                                  &account.secret);

        let get = GetCommand::<TimeStampRequest, Vec<UMSwapPosition>> { phantom: Default::default() };
        let positions = get.execute(info, Some(Default::default())).await.unwrap();
        for p in &positions {
            println!("symbol:{},持仓未实现盈亏:{},名义价值:{}", p.symbol, p.unrealized_profit, p.notional);
        }
    }
}

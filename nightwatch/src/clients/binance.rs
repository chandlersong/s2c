use crate::clients::binance_models::{BinanceBase, BinancePath, CommandInfo, NormalAPI};
use crate::errors::NightWatchError;
use crate::models::EmptyObject;
use crate::settings::Settings;
use lazy_static::lazy_static;
use log::trace;
use serde::de::DeserializeOwned;
use std::env;
use std::marker::PhantomData;
use url::Url;

lazy_static! {
    static ref CLIENT:  reqwest::Client = init_client();
}



pub(crate) async fn execute_ping() -> Result<(), NightWatchError> {
    let info = CommandInfo {
        base: BinanceBase::Normal,
        path: BinancePath::Normal(NormalAPI::PingAPI),
        sign: false,
        client: &CLIENT,
    };

    let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
    let _ = get.execute(info, EmptyObject {}).await?;
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

trait BNCommand<T, U> {
    async fn execute(&self, info: CommandInfo, data: T) -> Result<U, NightWatchError>;
}


pub struct GetCommand<T, U: DeserializeOwned> {
    phantom: PhantomData<(T, U)>,
}

impl<T, U: DeserializeOwned> BNCommand<T, U> for GetCommand<T, U> {
    async fn execute(&self, info: CommandInfo<'_>, data: T) -> Result<U, NightWatchError> {
        let mut url = Url::parse(&String::from(info.base)).expect("Invalid base URL");
        url.set_path(&String::from(&String::from(info.path)));
        let res = info.client.get(url).send().await?;
        trace!("Response: {:?} {}", res.version(), res.status());
        trace!("Headers: {:#?}\n", res.headers());
        let body = res.json().await?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::binance::CommandInfo;
    use crate::clients::binance_models::{BinanceBase, BinancePath, NormalAPI};
    use crate::models::EmptyObject;

    /**
    因为这里的方法，都是一些直接连接服务器的。所以都ignore了。需要去连接后面。
    **/

    #[ignore]
    #[tokio::test]
    async fn test_ping() {
        let info = CommandInfo {
            base: BinanceBase::Normal,
            path: BinancePath::Normal(NormalAPI::PingAPI),
            sign: false,
            client: &CLIENT,
        };

        let get = GetCommand::<EmptyObject, EmptyObject> { phantom: Default::default() };
        let x = get.execute(info, EmptyObject {}).await.unwrap();
        assert_eq!(x, EmptyObject {})
    }
}

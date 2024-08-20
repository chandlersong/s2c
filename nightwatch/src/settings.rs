use config::{Config, ConfigError, File};
use lazy_static::lazy_static;
use log::info;
use serde::Deserialize;
use std::env;

#[derive(Clone, Debug, Deserialize)]
#[allow(unused)]
pub struct Account {
    pub name: String,
    pub api_key: String,
    pub secret: String,
    pub funding_rate_arbitrage: Option<Vec<String>>,
    #[serde(default)]
    pub burning_free: bool, //是否燃烧降低手续费
}

lazy_static! {
   pub  static ref SETTING :  Settings = init_setting();
}

fn init_setting() -> Settings {
    let mut current_dir = env::current_dir().unwrap();
    current_dir.push("conf/Settings");
    let config_path = current_dir.to_str().unwrap();
    info!("working dir is {:?}",current_dir);
    let config_path = env::var("NIGHT_WATCH_CONFIG").unwrap_or_else(|_| String::from(config_path));
    info!("configuration path:{}", &config_path);
    Settings::new(&config_path).unwrap()
}


#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    pub proxy: Option<String>,
    pub accounts: Vec<Account>,
}

impl Settings {
    pub fn new(path: &str) -> Result<Self, ConfigError> {
        let s = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(File::with_name(path))
            .build()?;

        let proxy = s.get_string("proxy").map(Some).unwrap_or(None);

        // 获取所有 person 配置信息
        let accounts: Vec<Account> = s.get("account").unwrap();


        Ok(Settings {
            proxy,
            accounts,
        })
    }

    #[cfg(test)]
    pub fn get_account(&self, idx: usize) -> &Account {
        &self.accounts[idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_setting() {
        let setting = Settings::new("tests/Settings.toml").unwrap();
        assert_eq!(setting.accounts.len(), 2);

        let actual = setting.accounts.get(0).unwrap();
        assert_eq!(actual.name, "abc");
        assert_eq!(actual.api_key, "189rjfadoisfj8923fjio");
        assert_eq!(actual.secret, "bfsabfsbsfbsfbsfa31bw");
        assert_eq!(actual.burning_free, true);
        let coins = &actual.funding_rate_arbitrage;
        match coins {
            None => { assert!(false, "数组为空"); }
            Some(v) => { assert_eq!(v.len(), 3, "载入数量不对"); }
        }


        assert_eq!(setting.proxy, Some(String::from("http://localhost:7890")));
    }
}

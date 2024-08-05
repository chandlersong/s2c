use config::{Config, ConfigError, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(unused)]
struct Account {
    name: String,
    api_key: String,
    secret: String,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Settings {
    proxy: Option<String>,
    accounts: Vec<Account>,
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

        assert_eq!(setting.proxy, Some(String::from("http://localhost:7890")));
    }
}

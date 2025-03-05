use config::{Config, ConfigError, File};
use log::info;
use serde::Deserialize;
use std::env;
use std::sync::LazyLock;

pub static VARYS_CONFIG: LazyLock<VarysConfig> = LazyLock::new(|| {
    init_setting()
});


fn init_setting() -> VarysConfig {
    let mut current_dir = env::current_dir().unwrap();
    current_dir.push("conf/varys/Settings.toml");
    let config_path = current_dir.to_str().unwrap();
    let config_path = env::var("VARYS_CONFIG").unwrap_or_else(|_| String::from(config_path));
    info!("varys configuration path:{}", &config_path);
    VarysConfig::new(&config_path).unwrap()
}

#[derive(Debug, Deserialize)]
pub struct VarysSpotConfig {
    pub trade: Vec<String>,
    #[serde(rename = "miniticker")]
    pub mini_ticker: bool,
}


#[derive(Debug, Deserialize)]
pub struct VarysConfig {
    #[serde(rename = "dbPath")]
    pub db_path: String,
    pub spot: VarysSpotConfig,
}

impl VarysConfig {
    fn new(path: &str) -> Result<Self, ConfigError> {
        let s = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(File::with_name(path))
            .build()?;
        let db_path = s.get("dbPath")?;
        let spot: VarysSpotConfig = s.get("spot")?;
        Ok(Self {
            db_path,
            spot
        })
    }
}


#[cfg(test)]
mod tests {
    use crate::settings::VarysConfig;

    #[test]
    fn test_load_setting() {
        let setting = VarysConfig::new("tests/Settings.toml").unwrap();
        assert_eq!(setting.db_path, "test/db/all");

        let spot_config = &setting.spot;

        assert_eq!(spot_config.mini_ticker, true);
        let trade = &spot_config.trade;
        assert_eq!(trade.len(), 1, "载入数量不对");
        assert_eq!(trade[0], "BTCUSDT");
    }
}

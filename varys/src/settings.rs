use config::{Config, ConfigError, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VarysSpotConfig {
    pub trade: Vec<String>,
    #[serde(rename = "miniticker")]
    pub mini_ticker: bool,
}


#[derive(Debug, Deserialize)]
pub struct VarysConfig {
    pub spot: VarysSpotConfig,
}

impl VarysConfig {
    fn new(path: &str) -> Result<Self, ConfigError> {
        let s = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(File::with_name(path))
            .build()?;

        let spot: VarysSpotConfig = s.get("spot")?;
        Ok(Self {
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
        let spot_config = &setting.spot;

        assert_eq!(spot_config.mini_ticker, true);
        let trade = &spot_config.trade;
        assert_eq!(trade.len(), 1, "载入数量不对");
        assert_eq!(trade[0], "BTCUSDT");
    }
}

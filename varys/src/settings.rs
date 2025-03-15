use config::{Config, ConfigError, File};
use log::info;
use serde::Deserialize;
use std::sync::LazyLock;
use std::{env, fmt};

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
pub struct RobotConfig {
    
    pub client_id: String,
    
    pub chat_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct AllRobotConfig {
    pub ops: RobotConfig,
}

#[derive(Debug, Deserialize)]
pub struct DepthConfiguration {
    pub symbol: String,
    pub level: u8,
    pub frequency: u16,
}

impl fmt::Display for DepthConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "symbol {}, level : {}, frequency: {}",
               self.symbol, self.level, self.frequency)
    }
}

#[derive(Debug, Deserialize)]
pub struct VarysSpotConfig {
    pub trade: Option<Vec<String>>,
    pub mini_ticker: Option<bool>,
    pub depth: Option<Vec<DepthConfiguration>>,
}



#[derive(Debug, Deserialize)]
pub struct ExchangeData {
    pub spot: VarysSpotConfig,
}


#[derive(Debug, Deserialize)]
pub struct VarysConfig {
    pub db_path: String,
    pub binance: ExchangeData,
    pub robot: AllRobotConfig,
}

impl VarysConfig {
    fn new(path: &str) -> Result<Self, ConfigError> {
        let s = Config::builder()
            // Start off by merging in the "default" configuration file
            .add_source(File::with_name(path))
            .build()?;
        let db_path = s.get("db_path")?;
        let binance : ExchangeData = s.get("binance")?;

        let robot: AllRobotConfig = s.get("robot")?;
        Ok(Self {
            db_path,
            binance,
            robot
        })
    }
}


#[cfg(test)]
mod tests {
    use crate::settings::VarysConfig;

    #[test]
    fn test_load_setting_all() {
        let setting = VarysConfig::new("tests/all-Settings.toml").unwrap();
        assert_eq!(setting.db_path, "test/db/all");

        let binance_spot_config = &setting.binance.spot;

        match &binance_spot_config.mini_ticker {
            Some(mini_ticker) => {
                assert!(mini_ticker, "miniTicker should be true");
            }
            None => {
                panic!("mini_ticker为空");
            }
        }

        match &binance_spot_config.depth {
            Some(depth) => {
                assert_eq!(depth.len(), 1, "载入数量不对");
                let d = &depth[0];
                assert_eq!(d.symbol, "SOLUSDT", "depth symbol读取不对");
                assert_eq!(d.level, 20, "depth level读取不对");
                assert_eq!(d.frequency, 1000, "depth level读取不对");
            }
            None => {
                panic!("depth_100ms为空");
            }
        }


        match &binance_spot_config.trade {
            Some(trade) => {
                assert_eq!(trade.len(), 1, "载入数量不对");
                assert_eq!(trade[0], "BTCUSDT");
            }
            None => {
                panic!("trade为空");
            }
        }


        let robot = &setting.robot;
        let ops_robot = &robot.ops;
        assert_eq!(ops_robot.chat_id, 123, "ops chat id");
        assert_eq!(ops_robot.client_id, "abc", "ops client id");
    }


    #[test]
    fn test_load_setting_small() {
        let setting = VarysConfig::new("tests/small-Settings.toml").unwrap();
        assert_eq!(setting.db_path, "test/db/all");
        let robot = &setting.robot;
        let ops_robot = &robot.ops;
        assert_eq!(ops_robot.chat_id, 123, "ops chat id");
        assert_eq!(ops_robot.client_id, "abc", "ops client id");

        assert!(setting.binance.spot.trade.is_none());
        assert!(setting.binance.spot.mini_ticker.is_none());
        assert!(setting.binance.spot.depth.is_none());
    }
}

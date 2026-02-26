use config::Config;
use serde::Deserialize;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use yue::binance::websocket_actor::SpotStreamAccountWebsocketInfo;
use yue::tools::load_ed25519_signing_key;

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SecurityType {
    #[serde(rename = "HMAC")]
    HMAC,
    #[serde(rename = "Ed25519")]
    Ed25519,
}
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum AccountType {
    #[serde(rename = "BinanceNormal")]
    BinanceNormal, //币安一般账户
    #[serde(rename = "BinancePortfolio")]
    BinancePortfolio, //币安统一账户
}

/// 账户配置的认证类型枚举
///
/// ## 设计思路
/// - 想要把这个做成通用的。支持不同交易所。所以就这样来设计了。
///
#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub struct AccountConfig {
    pub account_name: String,
    pub api_key: String,
    pub value: String,
    pub secret_type: SecurityType,
    pub account_type: AccountType,
}

impl Into<SpotStreamAccountWebsocketInfo> for AccountConfig {
    fn into(self) -> SpotStreamAccountWebsocketInfo {
        match self.secret_type {
            SecurityType::Ed25519 => {
                let private_key = load_ed25519_signing_key(self.value.as_ref()).expect("加载私钥失败");
                SpotStreamAccountWebsocketInfo {
                    account_name: self.account_name.clone(),
                    api_key: self.api_key.clone(),
                    private_key,
                }
            }
            _ => {
                panic!("to SpotStreamAccountWebsocketInfo only support Ed25519 account");
            }
        }
    }
}

// Binance 配置结构体
#[derive(Deserialize, Debug, Clone)]
pub struct BinanceConfig {
    pub accounts: Option<Vec<AccountConfig>>,
    pub spot_stream: Option<SpotWebSocketStreamConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpotWebSocketStreamConfig {
    pub trade: Option<SpotTradeStreamConfig>,
    pub depth: Option<SpotDepthStreamConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpotDepthStreamConfig {
    pub enabled: Option<bool>,
    pub symbols: Vec<String>,
    pub update_speed: Option<String>, // "100ms" 或 "1000ms"
    pub levels: Option<u32>,          // 5/10/20/none
}

impl SpotDepthStreamConfig {
    /// 是否启用深度行情流（默认 true）
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// 获取更新速度，支持 "100ms" 或 "1000ms"（默认 "100ms"）
    pub fn update_speed(&self) -> String {
        self.update_speed.clone().unwrap_or_else(|| "100ms".to_string())
    }

    /// 获取订单簿档位数（默认 20）
    pub fn levels(&self) -> u32 {
        self.levels.unwrap_or(20)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpotTradeStreamConfig {
    pub enabled: Option<bool>,
    pub symbols: Vec<String>,
    pub batch_size: Option<usize>,
    pub flush_interval_ms: Option<u64>,
    pub retention_days: Option<u32>,
}

impl SpotTradeStreamConfig {
    /// 获取所有需要订阅的交易对（大写，去重）

    /// 获取 trade 批量大小（默认 100）
    pub fn batch_size(&self) -> usize {
        self.batch_size.unwrap_or(100)
    }

    pub fn flush_interval_ms(&self) -> u64 {
        self.flush_interval_ms.unwrap_or(1000)
    }

    /// 获取 trade 数据保留天数（默认 7）
    pub fn retention_days(&self) -> u32 {
        self.retention_days.unwrap_or(7)
    }
}

#[derive(Deserialize, Debug)]
pub struct AppConfig {
    #[serde(rename = "proxyUrl")]
    pub proxy_url: Option<String>,
    pub database: Option<DuckDBConfig>,
    pub data_retention_hours: Option<u64>,
    // Optional log level for the application. Example values: "off", "error", "warn", "info", "debug", "trace"
    #[serde(rename = "logLevel")]
    pub log_level: Option<String>,
    pub binance: Option<BinanceConfig>,
    pub data_integrity: Option<DataIntegrityConfig>,
}

impl AppConfig {
    pub fn get_data_integrity_config(&self) -> DataIntegrityConfig {
        self.data_integrity.clone().unwrap_or_default()
    }

    pub fn get_data_retention_hours(&self) -> u64 {
        self.data_retention_hours.unwrap_or(100000) // 默认7天
    }

    pub fn get_data_retention_ms(&self) -> u64 {
        self.get_data_retention_hours() * 60 * 60 * 1000
    }

    ///
    /// 获得数据保存的最早整点时间戳，单位毫秒
    ///
    /// 如果现在是18:05分，data_retention_hours是10。那么就是取8点的时间戳。
    /// 如果utc_now为None，则取现在的时间。否则就是取现在的now
    ///
    pub fn get_earliest_hour_time_ms(&self, utc_now: Option<u64>) -> u64 {
        // 以毫秒为单位的一小时常量
        const HOUR_MS: u64 = 3_600_000;

        // 获取当前时间（毫秒），优先使用传入的 utc_now，否则使用系统时间
        let now_ms: u64 = utc_now.unwrap_or_else(|| {
            let dur = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System time before UNIX_EPOCH");
            dur.as_millis() as u64
        });

        // 向下取整到当前整点（小时）
        let floored_hour_ms = (now_ms / HOUR_MS) * HOUR_MS;

        // 计算需要回退的毫秒数，使用饱和乘法防止溢出
        let retention_hours = self.get_data_retention_hours();
        let backoff_ms = retention_hours.saturating_mul(HOUR_MS);

        // 使用饱和减法防止下溢（如果 backoff_ms 大于 floored_hour_ms，则返回 0）
        floored_hour_ms.saturating_sub(backoff_ms)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct DuckDBConfig {
    pub path: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DataIntegrityConfig {
    // 默认1个小时
    pub startup_check_timeout_ms: u64,
    // linux corn的模式 默认 * 3-53/10 * * * * *。 每小时3分钟开始，然后没5分钟一次。
    pub periodic_check_interval_cron: String,
    pub repair_backoff: RepairBackoffConfig,
}

impl Default for DataIntegrityConfig {
    fn default() -> Self {
        DataIntegrityConfig {
            startup_check_timeout_ms: 3_600_000,
            periodic_check_interval_cron: "0 3-53/10 * * * * *".to_string(),
            repair_backoff: Default::default(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct RepairBackoffConfig {
    // 默认3
    pub max_retries: u32,
}

impl Default for RepairBackoffConfig {
    fn default() -> Self {
        RepairBackoffConfig { max_retries: 3 }
    }
}

pub(crate) static CONFIG: OnceLock<AppConfig> = OnceLock::new();

pub fn get_config() -> &'static AppConfig {
    CONFIG.get_or_init(|| {
        let default_path = "config.toml";
        let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| default_path.to_string());
        if !Path::new(&config_path).exists() {
            panic!("配置文件不存在: {}", config_path);
        }
        Config::builder()
            .add_source(config::File::with_name(&config_path))
            .add_source(config::Environment::with_prefix("YU").try_parsing(true).separator("_"))
            .build()
            .expect("Failed to build config")
            .try_deserialize::<AppConfig>()
            .expect("Failed to deserialize config")
    })
}

#[cfg(test)]
mod tests {
    use super::{AccountType, AppConfig, SecurityType};
    use config::Config;

    // 使用直接反序列化避免 OnceLock 缓存问题
    #[test]
    pub fn test_binance_websocket_config_all_deserialization() {
        let config_builder = Config::builder()
            .add_source(config::File::with_name("tests/config_test/config_all.yaml"))
            .build()
            .expect("Failed to build config");

        let app_config: AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");
        assert_eq!(app_config.get_data_retention_hours(), 99, "get_data_retention_hours 不正确");
        // 验证 spot_websocket 配置

        // 验证 binance_websocket 配置存在
        assert!(app_config.binance.is_some(), "binance 配置应该存在");

        let binance_config = app_config.binance.as_ref().unwrap();
        assert!(binance_config.spot_stream.is_some(), "spot_stream 配置应该存在");

        let spot_stream_config = binance_config.spot_stream.as_ref().unwrap();

        // 验证 trade 配置
        assert!(spot_stream_config.trade.is_some(), "trade 配置应该存在");
        let trade_config = spot_stream_config.trade.as_ref().unwrap();
        assert_eq!(trade_config.symbols.len(), 3, "trade symbols 应该有 3 个");
        assert!(trade_config.symbols.contains(&"BTCUSDT".to_string()), "应该包含 BTCUSDT");
        assert!(trade_config.symbols.contains(&"ETHUSDT".to_string()), "应该包含 ETHUSDT");
        assert!(trade_config.symbols.contains(&"BNBUSDT".to_string()), "应该包含 BNBUSDT");
        assert_eq!(trade_config.batch_size, Some(100), "trade batch_size 应该是 100");
        assert_eq!(trade_config.flush_interval_ms, Some(5000), "trade flush_interval_ms 应该是 5000");
        assert_eq!(trade_config.retention_days, Some(7), "trade retention_days 应该是 7");

        // 验证 depth 配置
        assert!(spot_stream_config.depth.is_some(), "depth 配置应该存在");
        let depth_config = spot_stream_config.depth.as_ref().unwrap();
        assert_eq!(depth_config.symbols.len(), 3, "depth symbols 应该有 3 个");
        assert!(depth_config.symbols.contains(&"BTCUSDT".to_string()), "应该包含 BTCUSDT");
        assert!(depth_config.symbols.contains(&"ETHUSDT".to_string()), "应该包含 ETHUSDT");
        assert!(depth_config.symbols.contains(&"BNBUSDT".to_string()), "应该包含 BNBUSDT");
        assert_eq!(depth_config.enabled, Some(true), "depth enabled 应该是 true");
        assert_eq!(depth_config.update_speed, Some("100ms".to_string()), "update_speed 应该是 100ms");
        assert_eq!(depth_config.levels, Some(20), "levels 应该是 20");

        let accounts = binance_config.accounts.as_ref().unwrap();

        assert_eq!(accounts.len(), 2, "accounts 应该有 2 个");

        let account_hmac = &accounts[0];
        // 验证第一个账户（HMAC 类型）
        assert_eq!(account_hmac.account_name, "account1", "第一个账户名称应该是 account1");
        assert_eq!(account_hmac.api_key, "test_api_key_1", "第一个账户 API Key 应该匹配");
        assert_eq!(account_hmac.value, "test_secret_key_1", "第一个账户 Secret Key 应该匹配");
        assert_eq!(account_hmac.secret_type, SecurityType::HMAC, "第一个账户 secret_type 应该匹配");
        assert_eq!(account_hmac.account_type, AccountType::BinanceNormal, "第一个账户 saccount_type 应该匹配");

        let account_ed25519 = &accounts[1];
        // 验证第一个账户（HMAC 类型）
        assert_eq!(account_ed25519.account_name, "account2", "第二个账户名称应该是 account2");
        assert_eq!(account_ed25519.api_key, "test_api_key_2", "第二个账户 API Key 应该匹配");
        assert_eq!(account_ed25519.value, "test_secret_key_2", "第二个账户私钥路径应该匹配");
        assert_eq!(account_ed25519.secret_type, SecurityType::Ed25519, "第二个账户 secret_type 应该匹配");
        assert_eq!(
            account_ed25519.account_type,
            AccountType::BinancePortfolio,
            "第二个账户 saccount_type 应该匹配"
        );
    }

    #[test]
    pub fn test_binance_websocket_config_min_deserialization() {
        let config_builder = Config::builder()
            .add_source(config::File::with_name("tests/config_test/config_min.yaml"))
            .build()
            .expect("Failed to build config");

        let app_config: AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        // 最小配置不应该包含 binance_websocket
        assert!(app_config.binance.is_none(), "最小配置不应该包含 binance_websocket");

        assert_eq!(app_config.get_data_retention_hours(), 100000, "get_data_retention_hours 默认值不正确");
        // 但应该包含基础配置
        assert!(app_config.database.is_some(), "database 配置应该存在");
        assert!(app_config.proxy_url.is_some(), "proxy_url 应该存在");
        assert!(app_config.log_level.is_some(), "log_level 应该存在");
    }

    #[test]
    fn test_data_integrity_config_deserialization() {
        let yaml = r#"
proxyUrl: "http://127.0.0.1:1087"
logLevel: "info"
data_integrity:
  startup_check_timeout_ms: 5000
  periodic_check_interval_cron: "*/5 * * * * * *"
  repair_backoff:
    max_retries: 5
"#;

        let config_builder = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .expect("Failed to build config");

        let app_config: AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        let di = app_config.data_integrity.expect("data_integrity should exist");
        assert_eq!(di.startup_check_timeout_ms, 5000);
        assert_eq!(di.periodic_check_interval_cron, "*/5 * * * * * *");

        let backoff = di.repair_backoff;
        assert_eq!(backoff.max_retries, 5);
    }

    #[test]
    fn test_data_integrity_config_defaults() {
        let yaml = r#"
proxyUrl: "http://127.0.0.1:1087"
logLevel: "info"
"#;

        let config_builder = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .expect("Failed to build config");

        let app_config: AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        let di = app_config.get_data_integrity_config();
        assert_eq!(di.startup_check_timeout_ms, 3_600_000);
        assert_eq!(di.periodic_check_interval_cron, "0 3-53/10 * * * * *");

        let backoff = di.repair_backoff;
        assert_eq!(backoff.max_retries, 3);
    }

    // 新增的确定性测试：
    #[test]
    fn test_get_earliest_hour_basic() {
        const HOUR_MS: u64 = 3_600_000;
        // 10:05 -> 向下取整到 10:00，然后回退 3 小时 -> 7:00
        let utc_now = 10 * HOUR_MS + 5 * 60 * 1000;
        let cfg = AppConfig {
            proxy_url: None,
            database: None,
            data_retention_hours: Some(3),
            log_level: None,
            binance: None,
            data_integrity: None,
        };
        let got = cfg.get_earliest_hour_time_ms(Some(utc_now));
        assert_eq!(got, 7 * HOUR_MS);
    }

    #[test]
    fn test_get_earliest_hour_retention_zero() {
        const HOUR_MS: u64 = 3_600_000;
        // 15:30 -> 向下取整 15:00，retention 0 -> 返回 15:00
        let utc_now = 15 * HOUR_MS + 30 * 60 * 1000;
        let cfg = AppConfig {
            proxy_url: None,
            database: None,
            data_retention_hours: Some(0),
            log_level: None,
            binance: None,
            data_integrity: None,
        };
        let got = cfg.get_earliest_hour_time_ms(Some(utc_now));
        assert_eq!(got, 15 * HOUR_MS);
    }

    #[test]
    fn test_get_earliest_hour_saturating_zero() {
        const HOUR_MS: u64 = 3_600_000;
        // 2:30 -> floored 2:00. retention 5 -> backoff 5h > 2h -> saturate to 0
        let utc_now = 2 * HOUR_MS + 30 * 60 * 1000;
        let cfg = AppConfig {
            proxy_url: None,
            database: None,
            data_retention_hours: Some(5),
            log_level: None,
            binance: None,
            data_integrity: None,
        };
        let got = cfg.get_earliest_hour_time_ms(Some(utc_now));
        assert_eq!(got, 0);
    }
}

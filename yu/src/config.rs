use config::Config;
use serde::Deserialize;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use yue::binance::websocket_handler::AccountWebsocketInfo;
use yue::tools::load_ed25519_signing_key;

#[derive(Deserialize, Debug, Clone)]
pub struct SpotWebSocketConfig {
    pub accounts: Vec<AccountConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AccountConfig {
    pub account_name: String,
    pub api_key: String,
    pub key_path: String,
}

impl Into<AccountWebsocketInfo> for AccountConfig {
    fn into(self) -> AccountWebsocketInfo {
        let private_key = load_ed25519_signing_key(self.key_path.as_ref()).expect("加载私钥失败");
        AccountWebsocketInfo {
            account_name: self.account_name.clone(),
            api_key: self.api_key.clone(),
            private_key: private_key,
        }
    }
}

// Binance WebSocket 配置结构体
#[derive(Deserialize, Debug, Clone)]
pub struct BinanceWebSocketConfig {
    pub spot_stream: Option<SpotWebSocketStreamConfig>,
    pub spot: Option<SpotWebSocketConfig>,
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
    // Optional log level for the application. Example values: "off", "error", "warn", "info", "debug", "trace"
    #[serde(rename = "logLevel")]
    pub log_level: Option<String>,
    pub binance_websocket: Option<BinanceWebSocketConfig>,
    pub data_integrity: Option<DataIntegrityConfig>,
}

impl AppConfig {
    pub fn get_data_integrity_config(&self) -> DataIntegrityConfig {
        self.data_integrity.clone().unwrap_or_default()
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
    // linux corn的模式 默认 7 * * * * * *。 每小时的7分钟完成。
    pub periodic_check_interval_cron: String,
    pub repair_backoff: RepairBackoffConfig,
}

impl Default for DataIntegrityConfig {
    fn default() -> Self {
        DataIntegrityConfig {
            startup_check_timeout_ms: 3_600_000,
            periodic_check_interval_cron: "* 7 * * * * *".to_string(),
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
    use config::Config;

    // 使用直接反序列化避免 OnceLock 缓存问题
    #[test]
    pub fn test_binance_websocket_config_all_deserialization() {
        let config_builder = Config::builder()
            .add_source(config::File::with_name("tests/config_test/config_all.yaml"))
            .build()
            .expect("Failed to build config");

        let app_config: super::AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        // 验证 spot_websocket 配置

        // 验证 binance_websocket 配置存在
        assert!(app_config.binance_websocket.is_some(), "binance_websocket 配置应该存在");

        let ws_config = app_config.binance_websocket.as_ref().unwrap();
        assert!(ws_config.spot_stream.is_some(), "spot_stream 配置应该存在");

        let spot_stream_config = ws_config.spot_stream.as_ref().unwrap();

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

        let spot_ws_config = ws_config.spot.as_ref().unwrap();

        assert_eq!(spot_ws_config.accounts.len(), 2, "accounts 应该有 2 个");

        // 验证第一个账户
        assert_eq!(spot_ws_config.accounts[0].account_name, "account1", "第一个账户名称应该是 account1");
        assert_eq!(spot_ws_config.accounts[0].api_key, "test_api_key_1", "第一个账户 API Key 应该匹配");
        assert_eq!(spot_ws_config.accounts[0].key_path, "test_secret_key_1", "第一个账户 Secret Key 应该匹配");

        // 验证第二个账户
        assert_eq!(spot_ws_config.accounts[1].account_name, "account2", "第二个账户名称应该是 account2");
        assert_eq!(spot_ws_config.accounts[1].api_key, "test_api_key_2", "第二个账户 API Key 应该匹配");
        assert_eq!(spot_ws_config.accounts[1].key_path, "test_secret_key_2", "第二个账户 Secret Key 应该匹配");
    }

    #[test]
    pub fn test_binance_websocket_config_min_deserialization() {
        let config_builder = Config::builder()
            .add_source(config::File::with_name("tests/config_test/config_min.yaml"))
            .build()
            .expect("Failed to build config");

        let app_config: super::AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        // 最小配置不应该包含 binance_websocket
        assert!(app_config.binance_websocket.is_none(), "最小配置不应该包含 binance_websocket");

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

        let app_config: super::AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

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

        let app_config: super::AppConfig = config_builder.try_deserialize().expect("Failed to deserialize config");

        let di = app_config.get_data_integrity_config();
        assert_eq!(di.startup_check_timeout_ms, 3_600_000);
        assert_eq!(di.periodic_check_interval_cron, "* 7 * * * * *");

        let backoff = di.repair_backoff;
        assert_eq!(backoff.max_retries, 3);
    }
}

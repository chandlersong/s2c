use config::Config;
use serde::Deserialize;
use std::env;
use std::path::Path;
use std::sync::OnceLock;

// Binance WebSocket 配置结构体
#[derive(Deserialize, Debug, Clone)]
pub struct BinanceWebSocketConfig {
    pub spot: Option<SpotWebSocketStreamConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpotWebSocketStreamConfig {
    pub trade: Option<StreamConfig>,
    pub depth_update: Option<StreamConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct StreamConfig {
    pub enabled: Option<bool>,
    pub symbols: Vec<String>,
    pub batch_size: Option<usize>,
    pub flush_interval_ms: Option<u64>,
    pub retention_days: Option<u32>,
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
}

#[derive(Deserialize, Debug)]
pub struct DuckDBConfig {
    pub path: Option<String>,
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
            .add_source(config::Environment::with_prefix("MINGLUAN").try_parsing(true).separator("_"))
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

        // 验证 binance_websocket 配置存在
        assert!(app_config.binance_websocket.is_some(), "binance_websocket 配置应该存在");

        let ws_config = app_config.binance_websocket.as_ref().unwrap();
        assert!(ws_config.spot.is_some(), "spot 配置应该存在");

        let spot_config = ws_config.spot.as_ref().unwrap();

        // 验证 trade 配置
        assert!(spot_config.trade.is_some(), "trade 配置应该存在");
        let trade_config = spot_config.trade.as_ref().unwrap();
        assert_eq!(trade_config.symbols.len(), 3, "trade symbols 应该有 3 个");
        assert!(trade_config.symbols.contains(&"BTCUSDT".to_string()), "应该包含 BTCUSDT");
        assert!(trade_config.symbols.contains(&"ETHUSDT".to_string()), "应该包含 ETHUSDT");
        assert!(trade_config.symbols.contains(&"BNBUSDT".to_string()), "应该包含 BNBUSDT");
        assert_eq!(trade_config.batch_size, Some(100), "trade batch_size 应该是 100");
        assert_eq!(trade_config.flush_interval_ms, Some(5000), "trade flush_interval_ms 应该是 5000");
        assert_eq!(trade_config.retention_days, Some(7), "trade retention_days 应该是 7");

        // 验证 depth_update 配置
        assert!(spot_config.depth_update.is_some(), "depth_update 配置应该存在");
        let depth_config = spot_config.depth_update.as_ref().unwrap();
        assert_eq!(depth_config.symbols.len(), 2, "depth_update symbols 应该有 2 个");
        assert!(depth_config.symbols.contains(&"BTCUSDT".to_string()), "应该包含 BTCUSDT");
        assert!(depth_config.symbols.contains(&"ETHUSDT".to_string()), "应该包含 ETHUSDT");
        assert_eq!(depth_config.batch_size, Some(50), "depth batch_size 应该是 50");
        assert_eq!(depth_config.flush_interval_ms, Some(3000), "depth flush_interval_ms 应该是 3000");
        assert_eq!(depth_config.retention_days, Some(3), "depth retention_days 应该是 3");
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
}

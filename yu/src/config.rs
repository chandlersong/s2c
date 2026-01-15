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
    pub trade: Option<SpotTradeStreamConfig>,
    pub depth: Option<SpotDepthStreamConfig>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct SpotDepthStreamConfig {
    pub enabled: Option<bool>,
    pub symbols: Vec<String>,
    pub update_speed: Option<String>,  // "100ms" 或 "1000ms"
    pub levels: Option<u32>,           // 5/10/20/none
    pub snapshot_limit: Option<u32>,   // 1000/5000
    pub cache_size: Option<usize>,     // 事件缓存上限
    pub max_query_levels: Option<u32>, // 查询最大档位
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

    /// 获取快照限制（默认 5000）
    pub fn snapshot_limit(&self) -> u32 {
        self.snapshot_limit.unwrap_or(5000)
    }

    /// 获取事件缓存上限（默认 1000）
    pub fn cache_size(&self) -> usize {
        self.cache_size.unwrap_or(1000)
    }

    /// 获取查询最大档位（默认 100）
    pub fn max_query_levels(&self) -> u32 {
        self.max_query_levels.unwrap_or(100)
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

        // 验证 depth 配置
        assert!(spot_config.depth.is_some(), "depth 配置应该存在");
        let depth_config = spot_config.depth.as_ref().unwrap();
        assert_eq!(depth_config.symbols.len(), 3, "depth symbols 应该有 3 个");
        assert!(depth_config.symbols.contains(&"BTCUSDT".to_string()), "应该包含 BTCUSDT");
        assert!(depth_config.symbols.contains(&"ETHUSDT".to_string()), "应该包含 ETHUSDT");
        assert!(depth_config.symbols.contains(&"BNBUSDT".to_string()), "应该包含 BNBUSDT");
        assert_eq!(depth_config.enabled, Some(true), "depth enabled 应该是 true");
        assert_eq!(depth_config.update_speed, Some("100ms".to_string()), "update_speed 应该是 100ms");
        assert_eq!(depth_config.levels, Some(20), "levels 应该是 20");
        assert_eq!(depth_config.snapshot_limit, Some(5000), "snapshot_limit 应该是 5000");
        assert_eq!(depth_config.cache_size, Some(1000), "cache_size 应该是 1000");
        assert_eq!(depth_config.max_query_levels, Some(100), "max_query_levels 应该是 100");
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
    fn test_spot_config_get_all_symbols() {
        let config = super::SpotWebSocketStreamConfig {
            trade: Some(super::SpotTradeStreamConfig {
                enabled: Some(true),
                symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
                batch_size: Some(100),
                flush_interval_ms: Some(5000),
                retention_days: Some(7),
            }),
            depth: None,
        };

        let symbols = config.trade.unwrap().symbols;
        assert_eq!(symbols.len(), 2); // BTCUSDT, ETHUSDT, BNBUSDT (去重)
        assert!(symbols.contains(&"BTCUSDT".to_string()));
        assert!(symbols.contains(&"ETHUSDT".to_string()));
    }

    #[test]
    fn test_spot_config_batch_sizes() {
        let config = super::SpotWebSocketStreamConfig {
            trade: Some(super::SpotTradeStreamConfig {
                enabled: Some(true),
                symbols: vec!["BTCUSDT".to_string()],
                batch_size: Some(200),
                flush_interval_ms: Some(5000),
                retention_days: Some(7),
            }),
            depth: None,
        };

        assert_eq!(config.trade.unwrap().batch_size(), 200);
    }
}

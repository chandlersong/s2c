use config::Config;
use serde::Deserialize;
use std::env;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Deserialize, Debug)]
pub struct AppConfig {
    #[serde(rename = "proxyUrl")]
    pub proxy_url: Option<String>,
    pub database: Option<DuckDBConfig>,
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
    use std::env;

    #[test]
    pub fn test_load_all() {
        env::set_var("CONFIG_PATH", "tests/config_test/config_all.yaml");
        let config = super::get_config();
        println!("{:?}", config);

        assert!(config.database.is_some());
    }

    #[test]
    pub fn test_load_min() {
        env::set_var("CONFIG_PATH", "tests/config_test/config_min.yaml");
        env::set_var("MINGLUAN_DATABASE_PATH", "test.db");

        let config = super::get_config();
        println!("{:?}", config);
        assert!(config.database.is_some());
        assert_eq!(config.database.as_ref().unwrap().path.as_ref().unwrap(), "test.db");
    }
}

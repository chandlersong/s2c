// 雪花算法简单集成示例
// 依赖 snowflake crate
// 用于生成唯一ID
use duckdb::DuckdbConnectionManager;
use r2d2::Pool;
use snowflake::SnowflakeIdGenerator;
use std::sync::{Mutex, OnceLock};

pub(crate) static SNOWFLAKE_GENERATOR: OnceLock<Mutex<SnowflakeIdGenerator>> = OnceLock::new();

pub fn get_snowflake_generator() -> &'static Mutex<SnowflakeIdGenerator> {
    //PLAN：多机部署时，work_id 和 datacenter_id 需要配置不同的值
    SNOWFLAKE_GENERATOR.get_or_init(|| Mutex::new(SnowflakeIdGenerator::new(1, 1)))
}

pub fn initial_memory_db() -> Pool<DuckdbConnectionManager> {
    let builder = Pool::builder()
        .max_size(2) // 最大连接数
        .min_idle(Some(1)) // 最小空闲连接数
        .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

    builder.build(DuckdbConnectionManager::memory().unwrap()).unwrap()
}

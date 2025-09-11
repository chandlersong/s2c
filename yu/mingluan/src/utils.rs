// 雪花算法简单集成示例
// 依赖 snowflake crate
// 用于生成唯一ID
use snowflake::SnowflakeIdGenerator;
use std::sync::{Mutex, OnceLock};

pub(crate) static SNOWFLAKE_GENERATOR: OnceLock<Mutex<SnowflakeIdGenerator>> = OnceLock::new();

pub fn get_snowflake_generator() -> &'static Mutex<SnowflakeIdGenerator> {
    //TODO：像超时这类进行配置。
    SNOWFLAKE_GENERATOR.get_or_init(|| Mutex::new(SnowflakeIdGenerator::new(1, 1)))
}

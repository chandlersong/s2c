// 雪花算法简单集成示例
// 依赖 snowflake crate
// 用于生成唯一ID
use snowflake::SnowflakeIdGenerator;
use std::sync::{Mutex, OnceLock};

pub(crate) static SNOWFLAKE_GENERATOR: OnceLock<Mutex<SnowflakeIdGenerator>> = OnceLock::new();

pub fn get_snowflake_generator() -> &'static Mutex<SnowflakeIdGenerator> {
    //PLAN：多机部署时，work_id 和 datacenter_id 需要配置不同的值
    SNOWFLAKE_GENERATOR.get_or_init(|| Mutex::new(SnowflakeIdGenerator::new(1, 1)))
}

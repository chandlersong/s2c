// 雪花算法简单集成示例
// 依赖 snowflake crate
// 用于生成唯一ID
use duckdb::DuckdbConnectionManager;
use r2d2::Pool;

pub fn initial_memory_db() -> Pool<DuckdbConnectionManager> {
    let builder = Pool::builder()
        .max_size(2) // 最大连接数
        .min_idle(Some(1)) // 最小空闲连接数
        .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

    builder.build(DuckdbConnectionManager::memory().unwrap()).unwrap()
}

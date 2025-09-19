use crate::errors::MingLuanError;
use duckdb::DuckdbConnectionManager;
use r2d2;
use r2d2::{Pool, PooledConnection};
use std::sync::OnceLock;

pub(crate) static CONNECTION_POOL: OnceLock<Pool<DuckdbConnectionManager>> = OnceLock::new();

pub fn get_connection_pool() -> &'static Pool<DuckdbConnectionManager> {
    //TODO 创建数据库
    // 1. 有配置读取数据库文件
    // 2. 没有配置就创建内存数据库
    // 3. 检查有没有创建表。否则就自动创建

    CONNECTION_POOL.get_or_init(|| {
        /* TODO
           1， 目前是内存数据库，改成文件数据库。并且位置从配置文件读取
           2.  修改配置项，比如设置多大的链接痴
        */
        let builder = r2d2::Pool::builder()
            .max_size(15) // 最大连接数
            .min_idle(Some(5)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间

        builder.build(DuckdbConnectionManager::memory().unwrap()).unwrap()
    })
}

#[derive(Clone)]
pub struct DBProvider {
    pool: Pool<DuckdbConnectionManager>,
}

impl DBProvider {
    #[cfg(test)]
    pub fn new(pool: Pool<DuckdbConnectionManager>) -> Self {
        DBProvider { pool }
    }

    pub fn acquire(&self) -> Result<PooledConnection<DuckdbConnectionManager>, MingLuanError> {
        Ok(self.pool.get()?)
    }
}

impl Default for DBProvider {
    fn default() -> Self {
        DBProvider { pool: get_connection_pool().clone() }
    }
}

use crate::config::get_config;
use crate::errors::MingLuanError;
use duckdb::DuckdbConnectionManager;
use r2d2;
use r2d2::{Pool, PooledConnection};
use std::sync::OnceLock;

pub(crate) static CONNECTION_POOL: OnceLock<Pool<DuckdbConnectionManager>> = OnceLock::new();

pub fn get_connection_pool() -> &'static Pool<DuckdbConnectionManager> {
    CONNECTION_POOL.get_or_init(|| {
        let builder = Pool::builder()
            .max_size(15) // 最大连接数
            .min_idle(Some(5)) // 最小空闲连接数
            .connection_timeout(std::time::Duration::from_secs(5)); // 连接超时时间
        builder.build(get_duck_connection_manager()).unwrap()
    })
}

fn get_duck_connection_manager() -> DuckdbConnectionManager {
    let db_config = get_config().database.as_ref();
    if let Some(db_config) = db_config {
        if let Some(path) = &db_config.path {
            return DuckdbConnectionManager::file(path).unwrap();
        }
    }
    DuckdbConnectionManager::memory().unwrap()
}

#[derive(Clone)]
pub struct DBProvider {
    pool: Pool<DuckdbConnectionManager>,
}

impl DBProvider {
    pub fn new(pool: Pool<DuckdbConnectionManager>) -> Self {
        DBProvider { pool }
    }

    pub fn acquire(&self) -> Result<PooledConnection<DuckdbConnectionManager>, MingLuanError> {
        Ok(self.pool.get()?)
    }
}

impl Default for DBProvider {
    fn default() -> Self {
        DBProvider {
            pool: get_connection_pool().clone(),
        }
    }
}

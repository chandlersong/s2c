use crate::config::get_config;
use crate::errors::YuError;
use duckdb::DuckdbConnectionManager;
use r2d2;
use r2d2::{Pool, PooledConnection};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::sync::OnceLock;

pub trait DuckDBPO: Debug + Clone + DeserializeOwned + 'static + Send + Sync {
    fn to_params(&self) -> duckdb::AppenderParamsFromIter<Vec<&dyn duckdb::ToSql>>;
}

pub(crate) static CONNECTION_POOL: OnceLock<Pool<DuckdbConnectionManager>> = OnceLock::new();

pub fn get_connection_pool() -> &'static Pool<DuckdbConnectionManager> {
    CONNECTION_POOL.get_or_init(|| {
        let builder = Pool::builder()
            .max_size(50) // 最大连接数
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

pub fn get_connection() -> Result<PooledConnection<DuckdbConnectionManager>, YuError> {
    DBProvider::default().acquire()
}
pub type DuckDbConnection = PooledConnection<DuckdbConnectionManager>;
#[derive(Clone)]
pub struct DBProvider {
    pool: Pool<DuckdbConnectionManager>,
}

impl DBProvider {
    pub fn new(pool: Pool<DuckdbConnectionManager>) -> Self {
        DBProvider { pool }
    }

    pub fn acquire(&self) -> Result<PooledConnection<DuckdbConnectionManager>, YuError> {
        const MAX_RETRIES: usize = 100;
        for attempt in 0..MAX_RETRIES {
            match self.pool.get() {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(e.into());
                    }
                    // 基于当前时间生成一个小的随机抖动，避免同时重试的冲突
                    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
                    let jitter = (nanos % 200) as u64; // 0..199 ms
                    let backoff_ms = 50 + (attempt as u64 * 50) + jitter; // 指数增长基数 + 抖动
                    std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                    continue;
                }
            }
        }
        // 理论上不会到达这里，但为满足签名返回一个错误
        Err(YuError::new("failed to acquire connection"))
    }
}

impl Default for DBProvider {
    fn default() -> Self {
        DBProvider {
            pool: get_connection_pool().clone(),
        }
    }
}

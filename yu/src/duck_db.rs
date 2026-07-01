use crate::config::get_config;
use duckdb::DuckdbConnectionManager;
use r2d2;
use r2d2::{Pool, PooledConnection};
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::sync::OnceLock;
use yue::errors::YueError;
use yue::query_message::DataSourceProviderTrait;

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

pub fn get_connection() -> Result<DuckDbConnection, YueError> {
    DuckDBDSProvider::default().acquire()
}
pub type DuckDbConnection = PooledConnection<DuckdbConnectionManager>;
#[derive(Clone)]
pub struct DuckDBDSProvider {
    pool: Pool<DuckdbConnectionManager>,
}

impl std::fmt::Debug for DuckDBDSProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // pool doesn't implement Debug in a useful way here; print a lightweight summary
        let info = format!("Pool(addr={:p})", &self.pool);
        f.debug_struct("DuckDBDSProvider").field("pool", &info).finish()
    }
}
impl DataSourceProviderTrait for DuckDBDSProvider {
    type Connection = DuckDbConnection;

    fn acquire(&self) -> Result<Self::Connection, YueError> {
        const MAX_RETRIES: usize = 100;
        for attempt in 0..MAX_RETRIES {
            match self.pool.get() {
                Ok(conn) => return Ok(conn),
                Err(e) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(YueError::new(
                            format!("failed to acquire connection after {} attempts: {}", MAX_RETRIES, e).as_str(),
                        ));
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
        Err(YueError::new("failed to acquire connection"))
    }
}

impl DuckDBDSProvider {
    pub fn new(pool: Pool<DuckdbConnectionManager>) -> Self {
        DuckDBDSProvider { pool }
    }
}

impl Default for DuckDBDSProvider {
    fn default() -> Self {
        DuckDBDSProvider {
            pool: get_connection_pool().clone(),
        }
    }
}

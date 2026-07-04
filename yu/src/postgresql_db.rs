use crate::config::get_config;
use crate::errors::YuError;
use sqlx::{postgres, PgPool};
use sqlx_core::pool::PoolConnection;
use tokio::sync::OnceCell;
use futures::executor::block_on;
use yue::errors::YueError;
use yue::query_message::DataSourceProviderTrait;

pub(crate) static SYNC_CLIENT_PG_POOL: OnceCell<PgPool> = OnceCell::const_new();

pub async fn get_sync_client_pg_pool() -> Result<&'static PgPool, YuError> {
    SYNC_CLIENT_PG_POOL
        .get_or_try_init(|| async {
            let app_config = get_config();
            let client_config = app_config.sync_client.as_ref().expect("sync_client config missing");
            client_config.pool_options().connect_with(client_config.connect_options()).await
        })
        .await
        .map_err(Into::into)
}

pub trait PostgresqlTableTrait: Send + Clone + 'static {
    fn table_name(&self) -> String;
    fn create_table_statement(&self) -> String;
}

struct PostgresqlDataSourceProvider {
    pool: &'static PgPool,
}

impl PostgresqlDataSourceProvider {
    pub fn new(pool: &'static PgPool) -> Self {
        Self { pool }
    }
}

impl DataSourceProviderTrait for PostgresqlDataSourceProvider {
    type Connection = PoolConnection<postgres::Postgres>;

    fn acquire(&self) -> Result<Self::Connection, YueError> {
        block_on(self.pool.acquire())
            .map_err(|e| YueError::new(&format!("Failed to acquire PostgreSQL connection: {}", e)))
    }
}

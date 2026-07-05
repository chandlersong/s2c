use crate::config::get_config;
use crate::errors::YuError;
use futures::executor::block_on;
use sqlx::PgPool;
use tokio::sync::OnceCell;

pub(crate) static SYNC_CLIENT_PG_POOL: OnceCell<PgPool> = OnceCell::const_new();

pub async fn get_sync_client_pg_pool() -> Result<PgPool, YuError> {
    SYNC_CLIENT_PG_POOL
        .get_or_try_init(|| async {
            let app_config = get_config();
            let client_config = app_config.sync_client.as_ref().expect("sync_client config missing");
            client_config.pool_options().connect_with(client_config.connect_options()).await
        })
        .await
        .map(|p| p.clone())
        .map_err(Into::into)
}

pub fn get_sync_client_pg_pool_sync() -> Result<PgPool, YuError> {
    block_on(get_sync_client_pg_pool())
}

pub trait PostgresqlTableTrait: Send + Clone + 'static {
    fn table_name(&self) -> &'static str;
    fn create_table_statement(&self) -> &'static str;
}

///
/// 一些对象，需要支持PostgreSQL的通过copy方式批量插入操作，需要实现这个trait
///
pub trait CopyInsertable: Send + Sync + 'static {
    fn columns() -> &'static str; // 如 "id,username,email,created_at"
    fn to_csv_row(&self) -> String; // 转为 CSV 格式的一行
}

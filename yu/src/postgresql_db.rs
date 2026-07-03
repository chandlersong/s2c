use crate::config::get_config;
use crate::errors::YuError;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tokio::sync::OnceCell;

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

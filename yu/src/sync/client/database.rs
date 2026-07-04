use crate::errors::YuError;
use crate::postgresql_db::{get_sync_client_pg_pool_sync, PostgresqlTableTrait};
use crate::sync::client::db_consts::ALL_CLIENT_POLYMARKET_TABLES;
use log::{error, info};
use sqlx_postgres::PgPool;

pub async fn initial_grpc_client_tables(option_pool: Option<PgPool>) -> Result<(), YuError> {
    let pool = option_pool.unwrap_or_else(|| get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool failed"));
    for table in ALL_CLIENT_POLYMARKET_TABLES.iter() {
        let create_sql = table.create_table_statement();
        // execute the whole SQL blob (may contain multiple statements); simpler and avoids slicing lifetimes
        let table_initial_stmt = create_sql.split(';');
        for sql in table_initial_stmt {
            match sqlx::query(sql).execute(&pool).await {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to execute create_sql for table {}:\nerror: {}", table.table_name(), e);
                }
            }
        }
    }
    info!("initial polymarket tables done");
    Ok(())
}

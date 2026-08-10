use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlTableTrait, get_sync_client_pg_pool_sync};
use crate::postgresql_db_tables::{PostgresqlBatchInsert, PostgresqlBatchInsertImpl};
use crate::sync::client::db_consts::ALL_CLIENT_TABLES;
use crate::sync::client::db_consts::ClientsTables::PriceHistory;
use crate::sync::client::po::polymarket::LocalPolyMarketHistoryPo;
use futures::executor::block_on;
use log::{error, info};
use sqlx::PgPool;
use std::sync::OnceLock;

pub async fn initial_grpc_client_tables(option_pool: Option<PgPool>) -> Result<(), YuError> {
    let pool = option_pool.unwrap_or_else(|| get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool failed"));
    for table in ALL_CLIENT_TABLES.iter() {
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

pub(crate) static POLYMARKET_PRICE_BATCH_INSERT: OnceLock<PostgresqlBatchInsert<LocalPolyMarketHistoryPo>> = OnceLock::new();

pub fn get_polymarket_price_batch_insert() -> PostgresqlBatchInsert<LocalPolyMarketHistoryPo> {
    POLYMARKET_PRICE_BATCH_INSERT
        .get_or_init(|| {
            let pool = get_sync_client_pg_pool_sync().expect("get_sync_client_pg_pool failed");
            let batch_insert = block_on(PostgresqlBatchInsertImpl::<LocalPolyMarketHistoryPo>::new(
                PriceHistory.table_name(),
                pool,
            ));
            batch_insert
        })
        .clone()
}

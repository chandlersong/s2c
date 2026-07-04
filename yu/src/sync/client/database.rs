use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlDataSourceProvider, PostgresqlTableTrait};
use crate::sync::client::db_consts::ALL_CLIENT_POLYMARKET_TABLES;
use log::{error, info};

pub async fn initial_tables(provider: Option<PostgresqlDataSourceProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| PostgresqlDataSourceProvider::default());
    let pool = db_provider.pool();
    for table in ALL_CLIENT_POLYMARKET_TABLES.iter() {
        let create_sql = table.create_table_statement();
        // execute the whole SQL blob (may contain multiple statements); simpler and avoids slicing lifetimes
        match sqlx::query(create_sql).execute(pool).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to execute create_sql for table {}:\nerror: {}", table.table_name(), e);
            }
        }
    }
    info!("initial polymarket tables done");
    Ok(())
}

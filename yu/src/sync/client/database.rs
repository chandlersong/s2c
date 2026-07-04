use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlDataSourceProvider, PostgresqlTableTrait};
use crate::sync::client::db_consts::ALL_CLIENT_POLYMARKET_TABLES;
use futures::executor::block_on;
use log::{error, info};

pub fn initial_tables(provider: Option<PostgresqlDataSourceProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| PostgresqlDataSourceProvider::default());
    let pool = db_provider.pool();
    for table in ALL_CLIENT_POLYMARKET_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                // make an owned String and leak it to 'static for sqlx::query which requires a 'static str
                // This leaks the SQL strings but it's acceptable for one-time initialization.
                let owned_sql = sql.to_string();
                let static_sql: &'static str = Box::leak(owned_sql.into_boxed_str());
                match block_on(async move { sqlx::query(static_sql).execute(pool).await }) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to execute sql: {}\nerror: {}", static_sql, e);
                    }
                }
            }
        }
    }
    info!("initial polymarket tables done");
    Ok(())
}

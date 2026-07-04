use crate::errors::YuError;
use crate::postgresql_db::{PostgresqlDataSourceProvider, PostgresqlTableTrait};
use crate::sync::client::db_consts::ALL_CLIENT_POLYMARKET_TABLES;
use log::{error, info};

pub async fn initial_tables(provider: Option<PostgresqlDataSourceProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| PostgresqlDataSourceProvider::default());
    let pool = db_provider.pool();
    for table in ALL_CLIENT_POLYMARKET_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                // sql is a &'static str (from constants), pass directly to sqlx::query
                match sqlx::query(sql).execute(pool).await {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to execute sql: {}\nerror: {}", sql, e);
                    }
                }
            }
        }
    }
    info!("initial polymarket tables done");
    Ok(())
}

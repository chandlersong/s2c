use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use crate::okx::duckdb_consts::ALL_OKX_TABLES;
use log::info;
use yue::query_message::DataSourceProviderTrait;

pub fn initial_okx_tables(provider: Option<DuckDBDSProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| DuckDBDSProvider::default());
    let conn = db_provider.acquire()?;
    for table in ALL_OKX_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                conn.execute(sql, [])?;
            }
        }
    }
    info!("initial okx tables done");
    Ok(())
}

use crate::binance::models::po::KlinePo;
use crate::duck_db::DBProvider;
use crate::duck_db_tables::DuckTableTableChannel;
use crate::errors::YuError;
use crate::polymarket::db_consts::ALL_POLYMARKET_TABLES;
use log::info;
use std::sync::OnceLock;

pub fn initial_tables(provider: Option<DBProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| DBProvider::default());
    let conn = db_provider.acquire()?;
    for table in ALL_POLYMARKET_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                conn.execute(sql, [])?;
            }
        }
    }
    info!("initial polymarket tables done");
    Ok(())
}

pub(crate) static POLYMARKET_PRICE_HISTORY: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

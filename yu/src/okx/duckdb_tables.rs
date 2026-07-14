use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::{DuckDBOneTable, DuckDbTableTrait, DuckTableTableChannel};
use crate::errors::YuError;
use crate::okx::duck_po::OkxKlinePo;
use crate::okx::duckdb_consts::{ALL_OKX_TABLES, OkxTables};
use log::info;
use std::sync::OnceLock;
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

pub(crate) static OKX_KLINE: OnceLock<DuckTableTableChannel<OkxKlinePo>> = OnceLock::new();

pub fn get_okx_kline_table() -> DuckTableTableChannel<OkxKlinePo> {
    OKX_KLINE
        .get_or_init(|| DuckDBOneTable::<OkxKlinePo, OkxTables>::start_new(OkxTables::Kline, None))
        .clone()
}

use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use duckdb::{DuckdbConnectionManager, params};
use r2d2::PooledConnection;

pub(crate) mod binance;
pub(crate) mod duck_db;
mod errors;
mod exchange;
#[cfg(test)]
pub mod test_utils;
mod utils;

struct Duck {
    id: i32,
    name: String,
}

fn main() -> Result<(), MingLuanError> {
    let acquire = DBProvider::default();
    let conn: PooledConnection<DuckdbConnectionManager> = acquire.acquire()?;

    // let manager = SpotKlineRefresh::new(conn);
    conn.execute(
        "CREATE TABLE ducks (id INTEGER PRIMARY KEY, name TEXT)",
        [], // empty list of parameters
    )
    .unwrap();

    conn.execute_batch(
        r#"
        INSERT INTO ducks (id, name) VALUES (1, 'Donald Duck');
        INSERT INTO ducks (id, name) VALUES (2, 'Scrooge McDuck');
        "#,
    )
    .unwrap();

    conn.execute(
        "INSERT INTO ducks (id, name) VALUES (?, ?)",
        params![3, "Darkwing Duck"],
    )
    .unwrap();

    let ducks = conn
        .prepare("FROM ducks")
        .unwrap()
        .query_map([], |row| {
            Ok(Duck {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })
        .unwrap()
        .collect::<duckdb::Result<Vec<_>>>()
        .unwrap();

    for duck in ducks {
        println!("{}) {}", duck.id, duck.name);
    }
    Ok(())
}

use crate::binance::update_manager::BinanceUpdateManager;
use crate::datasource::get_connection_pool;
use duckdb::DuckdbConnectionManager;
use r2d2::PooledConnection;

pub(crate) mod binance;
pub(crate) mod datasource;
mod exchange;
#[cfg(test)]
pub mod test_utils;

// struct Duck {
//     id: i32,
//     name: String,
// }

fn main() {
    let conn: PooledConnection<DuckdbConnectionManager> = get_connection_pool().get().unwrap();

    let manager = BinanceUpdateManager::new(conn);
    // conn.execute(
    //     "CREATE TABLE ducks (id INTEGER PRIMARY KEY, name TEXT)",
    //     [], // empty list of parameters
    // )
    // .unwrap();
    //
    // conn.execute_batch(
    //     r#"
    //     INSERT INTO ducks (id, name) VALUES (1, 'Donald Duck');
    //     INSERT INTO ducks (id, name) VALUES (2, 'Scrooge McDuck');
    //     "#,
    // )
    // .unwrap();
    //
    // conn.execute(
    //     "INSERT INTO ducks (id, name) VALUES (?, ?)",
    //     params![3, "Darkwing Duck"],
    // )
    // .unwrap();
    //
    // let ducks = conn
    //     .prepare("FROM ducks")
    //     .unwrap()
    //     .query_map([], |row| {
    //         Ok(Duck {
    //             id: row.get(0)?,
    //             name: row.get(1)?,
    //         })
    //     })
    //     .unwrap()
    //     .collect::<duckdb::Result<Vec<_>>>()
    //     .unwrap();
    //
    // for duck in ducks {
    //     println!("{}) {}", duck.id, duck.name);
    // }
}

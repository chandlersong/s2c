use crate::datasource::get_connection_pool;
use duckdb::arrow::array::Datum;
use duckdb::{Connection, DuckdbConnectionManager, params};
use std::thread;

pub mod datasource;

struct Duck {
    id: i32,
    name: String,
}

fn main() {
    let conn = get_connection_pool().get().unwrap();

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
}

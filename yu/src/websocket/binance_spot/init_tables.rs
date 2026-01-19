use crate::duck_db::DBProvider;
use crate::errors::YuError;
use duckdb::Connection;
use log::info;

// ============================================================================
// AccountSync 表管理
// ============================================================================

const CREATE_ACCOUNT_BALANCE_SPOT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS account_balance_spot (
    account_id TEXT NOT NULL,
    asset TEXT NOT NULL,
    free DOUBLE NOT NULL,
    locked DOUBLE NOT NULL,
    event_time TIMESTAMP NOT NULL,
    source_exchange TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    PRIMARY KEY (account_id, asset, event_time)
)
"#;

const CREATE_ORDER_EVENTS_SPOT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS order_events_spot (
    account_id TEXT NOT NULL,
    symbol TEXT NOT NULL,
    order_id TEXT NOT NULL,
    client_order_id TEXT NOT NULL,
    status TEXT NOT NULL,
    side TEXT NOT NULL,
    type TEXT NOT NULL,
    price DOUBLE NOT NULL,
    qty DOUBLE NOT NULL,
    exec_qty DOUBLE NOT NULL,
    last_exec_price DOUBLE NOT NULL,
    event_time TIMESTAMP NOT NULL,
    source_exchange TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    PRIMARY KEY (account_id, order_id, event_time)
)
"#;

const CREATE_ORDER_EVENTS_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_order_events_symbol_time
ON order_events_spot (symbol, event_time)
"#;

pub fn create_spot_stream_tables() -> Result<(), YuError> {
    let conn = DBProvider::default().acquire()?;

    let trade_table = r#"
    CREATE TABLE IF NOT EXISTS bn_spot_trade (
        id BIGINT NOT NULL PRIMARY KEY,
        event_time BIGINT NOT NULL,
        symbol VARCHAR NOT NULL,
        trade_id BIGINT NOT NULL,
        price DECIMAL(20,8) NOT NULL,
        qty DECIMAL(20,8) NOT NULL,
        trade_time BIGINT,
        is_buyer_maker BOOLEAN,
        created_at BIGINT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_bn_spot_trade_symbol_time ON bn_spot_trade(symbol, created_at DESC);
    "#;

    for stmt in trade_table.split(';') {
        let sql = stmt.trim();
        if !sql.is_empty() {
            conn.execute(sql, [])?;
        }
    }

    info!("bn_spot tables ensured");
    Ok(())
}

pub fn create_spot_websocket_tables(conn_out: Option<&Connection>) -> Result<(), YuError> {
    let owned_conn;
    let conn = match conn_out {
        Some(conn) => conn,
        None => {
            owned_conn = DBProvider::default().acquire()?;
            &owned_conn
        }
    };

    for stmt in [
        CREATE_ACCOUNT_BALANCE_SPOT_TABLE,
        CREATE_ORDER_EVENTS_SPOT_TABLE,
        CREATE_ORDER_EVENTS_INDEX,
    ] {
        let sql = stmt.trim();
        if !sql.is_empty() {
            conn.execute(sql, [])?;
        }
    }

    info!("spot websocket tables ensured");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use duckdb::Connection;

    fn create_test_connection() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn test_create_account_sync_tables() {
        let conn = create_test_connection();
        let result = create_spot_websocket_tables(Some(&conn));
        assert!(result.is_ok());

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'account_balance_spot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'order_events_spot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

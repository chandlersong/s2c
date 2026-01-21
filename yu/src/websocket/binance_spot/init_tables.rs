use crate::duck_db::DBProvider;
use crate::errors::YuError;
use duckdb::Connection;
use log::info;

// ============================================================================
// AccountSync 表管理
// ============================================================================

const CREATE_BN_ORDER_EVENTS_SPOT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_order_events_spot (
    event TEXT NOT NULL,
    event_time BIGINT NOT NULL,
    symbol TEXT NOT NULL,
    client_order_id TEXT NOT NULL,
    side TEXT NOT NULL,
    order_type TEXT NOT NULL,
    time_in_force TEXT NOT NULL,
    order_qty DOUBLE NOT NULL,
    order_price DOUBLE NOT NULL,
    stop_price DOUBLE NOT NULL,
    iceberg_qty DOUBLE NOT NULL,
    order_list_id BIGINT NOT NULL,
    original_client_order_id TEXT NOT NULL,
    execution_type TEXT NOT NULL,
    order_status TEXT NOT NULL,
    reject_reason TEXT NOT NULL,
    order_id BIGINT NOT NULL,
    last_executed_qty DOUBLE NOT NULL,
    cumulative_filled_qty DOUBLE NOT NULL,
    last_executed_price DOUBLE NOT NULL,
    commission_amount DOUBLE NOT NULL,
    commission_asset TEXT,
    trade_time BIGINT NOT NULL,
    trade_id BIGINT,
    stp BIGINT,
    order_creation_time BIGINT NOT NULL,
    is_working BOOLEAN NOT NULL,
    is_maker BOOLEAN NOT NULL,
    is_best_match BOOLEAN NOT NULL,
    order_create_time BIGINT NOT NULL,
    cumulative_quote_qty DOUBLE NOT NULL,
    last_quote_qty DOUBLE NOT NULL,
    quote_order_quantity DOUBLE NOT NULL,
    working_time BIGINT NOT NULL,
    self_trade_prevention_mode TEXT NOT NULL,
    trailing_delta DOUBLE,
    trailing_time BIGINT,
    strategy_id BIGINT,
    strategy_type BIGINT,
    prevented_quantity DOUBLE,
    last_prevented_quantity DOUBLE,
    trade_group_id BIGINT,
    counter_order_id BOOLEAN,
    counter_symbol TEXT,
    prevented_execution_quantity DOUBLE,
    prevented_execution_price DOUBLE,
    prevented_execution_quote_qty DOUBLE,
    match_type TEXT,
    allocation_id BIGINT,
    working_floor TEXT,
    used_sor BOOLEAN,
    pegged_price_type TEXT,
    pegged_offset_type TEXT,
    pegged_offset_value BIGINT,
    pegged_price DOUBLE,
    PRIMARY KEY (order_id, event_time)
)
"#;

const CREATE_ORDER_EVENTS_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_order_events_symbol_time
ON bn_order_events_spot (symbol, event_time DESC)
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

    for stmt in [CREATE_BN_ORDER_EVENTS_SPOT_TABLE, CREATE_ORDER_EVENTS_INDEX] {
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
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'bn_order_events_spot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'bn_order_events_spot'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

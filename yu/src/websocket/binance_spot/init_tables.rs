use crate::duck_db::DBProvider;
use crate::errors::YuError;
use log::info;

pub fn create_tables() -> Result<(), YuError> {
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

    // let depth_table = r#"
    // CREATE TABLE IF NOT EXISTS bn_spot_depth (
    //     id BIGINT NOT NULL PRIMARY KEY
    //     event_time BIGINT NOT NULL,
    //     symbol VARCHAR NOT NULL,
    //     first_update_id BIGINT,
    //     prev_final_update_id BIGINT,
    //     bids_json BLOB,
    //     asks_json BLOB,
    //     created_at BIGINT NOT NULL
    // );
    // CREATE INDEX IF NOT EXISTS idx_bn_spot_depth_symbol_time ON bn_spot_depth(symbol, created_at DESC);
    // "#;

    for stmt in trade_table.split(';') {
        let sql = stmt.trim();
        if !sql.is_empty() {
            conn.execute(sql, [])?;
        }
    }

    // for stmt in depth_table.split(';') {
    //     let sql = stmt.trim();
    //     if !sql.is_empty() {
    //         conn.execute(sql, [])?;
    //     }
    // }

    info!("bn_spot tables ensured");
    Ok(())
}

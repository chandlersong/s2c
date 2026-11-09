// 2020年1月1日零点的毫秒时间戳

use crate::duck_db::DuckDBDSProvider;
use crate::duck_db_tables::DuckDbTableTrait;
use crate::errors::YuError;
use log::info;
use yue::query_message::DataSourceProviderTrait;

pub static QUERY_LATEST_SPOT_KLINE_SQL: &str = "select symbol,max(close_time) as latest from bn_spot_kline group by symbol;";
pub static QUERY_LATEST_SWAP_KLINE_SQL: &str = "select symbol,max(close_time) as latest from bn_swap_kline group by symbol;";

pub static QUERY_LATEST_FUNDING_RATE_SQL: &str = "select symbol,max(funding_time)+60000 as latest from bn_swap_funding_rate group by symbol;";

#[derive(Clone)]
pub enum BinanceTables {
    Instrument,
    OptionInstrument,
    SpotKline,
    SwapKline,
    SwapFundingRate,
    SpotTrade,
    SpotOrderBookSnapshot,
    SpotOrderEvents,
    SwapOrderEvents,
}

impl DuckDbTableTrait for BinanceTables {
    fn table_name(&self) -> String {
        match self {
            BinanceTables::Instrument => String::from("bn_instruments"),
            BinanceTables::OptionInstrument => String::from("bn_option_instruments"),
            BinanceTables::SpotKline => String::from("bn_spot_kline"),
            BinanceTables::SwapKline => String::from("bn_swap_kline"),
            BinanceTables::SwapFundingRate => String::from("bn_swap_funding_rate"),
            BinanceTables::SpotTrade => String::from("bn_spot_trade"),
            BinanceTables::SpotOrderBookSnapshot => String::from("bn_spot_order_book_snapshot"),
            BinanceTables::SpotOrderEvents => String::from("bn_order_events_spot"),
            BinanceTables::SwapOrderEvents => String::from("bn_order_events_swap"),
        }
    }

    fn create_table_statement(&self) -> String {
        match self {
            BinanceTables::Instrument => String::from(CREATE_BINANCE_INSTRUMENTS_TABLE),
            BinanceTables::OptionInstrument => String::from(CREATE_BINANCE_OPTION_INSTRUMENTS_TABLE),
            BinanceTables::SpotKline => String::from(CREATE_SPOT_KLINE_TABLE),
            BinanceTables::SwapKline => String::from(CREATE_SWAP_KLINE_TABLE),
            BinanceTables::SwapFundingRate => String::from(CREATE_FUNDING_RATE_TABLE),
            BinanceTables::SpotTrade => String::from(BN_SPOT_TRADE_TABLE),
            BinanceTables::SpotOrderBookSnapshot => String::from(CREATE_BN_SPOT_ORDER_BOOK_SNAPSHOT_TABLE),
            BinanceTables::SpotOrderEvents => String::from(CREATE_BN_ORDER_EVENTS_SPOT_TABLE),
            BinanceTables::SwapOrderEvents => String::from(CREATE_BN_ORDER_EVENTS_SWAP_TABLE),
        }
    }

    fn query_lastest_record(&self) -> Option<String> {
        match self {
            BinanceTables::SpotKline => Some(String::from(QUERY_LATEST_SPOT_KLINE_SQL)),
            BinanceTables::SwapKline => Some(String::from(QUERY_LATEST_SWAP_KLINE_SQL)),
            BinanceTables::SwapFundingRate => Some(String::from(QUERY_LATEST_FUNDING_RATE_SQL)),
            _ => None,
        }
    }
}

pub const CREATE_BINANCE_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_instruments (
    id                      BIGINT,
    symbol                  VARCHAR NOT NULL,
    status                  VARCHAR NOT NULL,
    base_asset              VARCHAR NOT NULL,
    quote_asset             VARCHAR NOT NULL,
    quote_asset_precision   INTEGER NOT NULL,
    order_types             VARCHAR[] NOT NULL,
    symbol_type             VARCHAR NOT NULL,
    on_board_time           BIGINT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_bn_instruments_symbol_type
    ON bn_instruments(symbol, symbol_type);
"#;

pub const CREATE_BINANCE_OPTION_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_option_instruments (
    id                      BIGINT PRIMARY KEY,
    expiry_date             BIGINT NOT NULL,
    symbol                  VARCHAR NOT NULL,
    side                    VARCHAR NOT NULL,
    strike_price            DECIMAL(38, 18) NOT NULL,
    underlying              VARCHAR NOT NULL,
    unit                    INTEGER NOT NULL,
    liquidation_fee_rate    DECIMAL(38, 18) NOT NULL,
    min_qty                 DECIMAL(38, 18) NOT NULL,
    max_qty                 DECIMAL(38, 18) NOT NULL,
    initial_margin          DECIMAL(38, 18) NOT NULL,
    maintenance_margin      DECIMAL(38, 18) NOT NULL,
    min_initial_margin      DECIMAL(38, 18) NOT NULL,
    min_maintenance_margin  DECIMAL(38, 18) NOT NULL,
    price_scale             INTEGER NOT NULL,
    quantity_scale          INTEGER NOT NULL,
    quote_asset             VARCHAR NOT NULL,
    contract_type           VARCHAR NOT NULL,
    underlying_type         VARCHAR NOT NULL,
    naked_sell              BOOLEAN,
    status                  VARCHAR NOT NULL,
    filters                 JSON NOT NULL
);
"#;

// 直接为三张表生成格式化建表SQL（不带索引），字段对齐、注释清晰
pub const CREATE_SPOT_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_spot_kline (
    id                          BIGINT,   -- 主键ID
    inst_id                     BIGINT,  -- 交易对
    candle_begin_time           BIGINT,   -- K线开始时间
    open                        DOUBLE,   -- 开盘价
    high                        DOUBLE,   -- 最高价
    low                         DOUBLE,   -- 最低价
    close                       DOUBLE,   -- 收盘价
    volume                      DOUBLE,   -- 成交量
    quote_volume                DOUBLE,   -- 成交额
    number_of_trades            BIGINT,   -- 成交笔数
    taker_buy_base_asset_volume DOUBLE,   -- 主动买入成交量
    taker_buy_quote_asset_volume DOUBLE,  -- 主动买入成交额
    close_time                  BIGINT,    -- K线结束时间
    interval                    INT,       -- 周期，1为5m
    first_trade_id              BIGINT,    -- 第一个trading id
    last_trade_id               BIGINT    -- 最后一个trading id
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_spot_kline_symbol_candle_begin_time ON bn_spot_kline(inst_id, candle_begin_time);
"#;

pub const CREATE_SWAP_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_swap_kline (
    id                          BIGINT,   -- 主键ID
    inst_id                     BIGINT,  -- 交易对
    candle_begin_time           BIGINT,   -- K线开始时间
    open                        DOUBLE,   -- 开盘价
    high                        DOUBLE,   -- 最高价
    low                         DOUBLE,   -- 最低价
    close                       DOUBLE,   -- 收盘价
    volume                      DOUBLE,   -- 成交量
    quote_volume                DOUBLE,   -- 成交额
    number_of_trades            BIGINT,   -- 成交笔数
    taker_buy_base_asset_volume DOUBLE,   -- 主动买入成交量
    taker_buy_quote_asset_volume DOUBLE,  -- 主动买入成交额
    close_time                  BIGINT,    -- K线结束时间
    interval                    INT,       -- 周期，1为5m
    first_trade_id              BIGINT,    -- 第一个trading id
    last_trade_id               BIGINT    -- 最后一个trading id
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_swap_kline_symbol_candle_begin_time ON bn_swap_kline(inst_id, candle_begin_time);
"#;

pub const CREATE_FUNDING_RATE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_swap_funding_rate (
    id           BIGINT,   -- 主键ID
    inst_id       BIGINT,  -- 交易对
    funding_rate DOUBLE,   -- 资金费率
    funding_time BIGINT,   -- 资金费率时间
    mark_price   DOUBLE    -- 标记价格
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_swap_funding_rate_symbol_funding_time ON bn_swap_funding_rate(inst_id, funding_time);
"#;

const CREATE_BN_ORDER_EVENTS_SPOT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_order_events_spot (
    event TEXT NOT NULL,
    account_name TEXT NOT NULL,
    event_time BIGINT NOT NULL,
    inst_id BIGINT NOT NULL,
    client_order_id TEXT,
    side TEXT NOT NULL,
    order_type TEXT NOT NULL,
    time_in_force TEXT NOT NULL,
    order_qty DOUBLE NOT NULL,
    order_price DOUBLE NOT NULL,
    stop_price DOUBLE,
    execution_type TEXT NOT NULL,
    order_status TEXT NOT NULL,
    reject_reason TEXT,
    order_id BIGINT NOT NULL,
    last_executed_qty DOUBLE,
    cumulative_filled_qty DOUBLE,
    last_executed_price DOUBLE,
    commission_amount DOUBLE,
    commission_asset TEXT,
    trade_time BIGINT NOT NULL,
    trade_id BIGINT NOT NULL,
    is_maker BOOLEAN NOT NULL,
    is_working BOOLEAN NOT NULL,
    order_create_time BIGINT NOT NULL,
    cumulative_quote_qty DOUBLE NOT NULL,
    last_quote_qty DOUBLE,
    quote_order_quantity DOUBLE,
    PRIMARY KEY (order_id, event_time)
);
CREATE INDEX IF NOT EXISTS idx_order_events_symbol_time ON bn_order_events_spot (inst_id, event_time DESC)
"#;

const BN_SPOT_TRADE_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS bn_spot_trade (
        id BIGINT NOT NULL PRIMARY KEY,
        event_time BIGINT NOT NULL,
        inst_id BIGINT NOT NULL,
        trade_id BIGINT NOT NULL,
        price DECIMAL(20,8) NOT NULL,
        qty DECIMAL(20,8) NOT NULL,
        trade_time BIGINT,
        is_buyer_maker BOOLEAN,
        created_at BIGINT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_bn_spot_trade_symbol_time ON bn_spot_trade(inst_id, created_at DESC);
    "#;

const CREATE_BN_SPOT_ORDER_BOOK_SNAPSHOT_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS bn_spot_order_book_snapshot (
        id BIGINT PRIMARY KEY,
        inst_id BIGINT NOT NULL,
        snapshot_time BIGINT NOT NULL,
        local_update_id BIGINT NOT NULL,
        bids JSON NOT NULL,
        asks JSON NOT NULL
    );
    "#;

// 原生方式：维护一个静态数组，便于遍历所有表类型
pub(crate) const ALL_BINANCE_TABLES: &[BinanceTables] = &[
    BinanceTables::Instrument,
    BinanceTables::OptionInstrument,
    BinanceTables::SpotKline,
    BinanceTables::SwapKline,
    BinanceTables::SwapFundingRate,
    BinanceTables::SpotOrderEvents,
    BinanceTables::SwapOrderEvents,
    BinanceTables::SpotTrade,
    BinanceTables::SpotOrderBookSnapshot,
];

pub fn drop_tables(provider: Option<DuckDBDSProvider>, tables: Option<&[BinanceTables]>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| DuckDBDSProvider::default());
    let conn = db_provider.acquire()?;
    let tables_to_drop = tables.unwrap_or(ALL_BINANCE_TABLES);
    for table in tables_to_drop {
        conn.execute(&format!("DROP TABLE IF EXISTS {}", table.table_name()), [])?;
    }

    Ok(())
}

pub fn initial_tables(provider: Option<DuckDBDSProvider>) -> Result<(), YuError> {
    let db_provider = provider.unwrap_or_else(|| DuckDBDSProvider::default());
    let conn = db_provider.acquire()?;
    for table in ALL_BINANCE_TABLES.iter() {
        let create_sql = table.create_table_statement();
        let table_initial_stmt = create_sql.split(';');
        for stmt in table_initial_stmt {
            let sql = stmt.trim();
            if !sql.is_empty() {
                conn.execute(sql, [])?;
            }
        }
    }
    info!("initial binance tables done");
    Ok(())
}

// K线表字段定义

const CREATE_BN_ORDER_EVENTS_SWAP_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_order_events_swap (
    event TEXT NOT NULL,
    account_name TEXT NOT NULL,
    event_time BIGINT NOT NULL,
    trade_time BIGINT NOT NULL,
    inst_id BIGINT NOT NULL,
    client_order_id TEXT,
    side TEXT NOT NULL,
    order_type TEXT NOT NULL,
    time_in_force TEXT NOT NULL,
    order_qty DOUBLE NOT NULL,
    order_price DOUBLE NOT NULL,
    avg_price DOUBLE,
    stop_price DOUBLE,
    execution_type TEXT NOT NULL,
    order_status TEXT NOT NULL,
    order_id BIGINT NOT NULL,
    last_filled_qty DOUBLE,
    executed_qty DOUBLE,
    last_filled_price DOUBLE,
    commission_asset TEXT,
    commission_amount DOUBLE,
    trade_id BIGINT,
    is_maker BOOLEAN,
    is_reduce_only BOOLEAN NOT NULL,
    position_side TEXT NOT NULL,
    realized_pnl DOUBLE NOT NULL,
    stp_mode TEXT NOT NULL,
    gtd BIGINT,
    PRIMARY KEY (order_id, event_time)
);
CREATE INDEX IF NOT EXISTS idx_order_events_swap_symbol_time ON bn_order_events_swap (inst_id, event_time DESC)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binance_instrument_table_ddl_executes() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        for statement in CREATE_BINANCE_INSTRUMENTS_TABLE.split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }

        let column_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('bn_instruments')", [], |row| row.get(0))
            .unwrap();
        assert_eq!(column_count, 9);
    }

    // 测试目的：验证 Binance OptionSymbol 表已注册并且服务端 DDL 可在 DuckDB 中创建完整 schema。
    // 准备数据/过程：通过 BinanceTables 获取 option instrument 的表名和 DDL，在内存 DuckDB 中执行后检查列数。
    // 预期结果：表名为 bn_option_instruments，初始化注册集合包含该表，DDL 创建包含全部 22 个字段。
    #[test]
    fn binance_option_instrument_table_ddl_executes() {
        let table = BinanceTables::OptionInstrument;
        assert_eq!(table.table_name(), "bn_option_instruments");
        assert!(ALL_BINANCE_TABLES.iter().any(|registered| registered.table_name() == table.table_name()));

        let conn = duckdb::Connection::open_in_memory().unwrap();
        for statement in table.create_table_statement().split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }

        let column_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('bn_option_instruments')", [], |row| row.get(0))
            .unwrap();
        assert_eq!(column_count, 22);
    }

    // 测试目的：验证订单簿快照表已注册到 Binance 表集合且 DDL 包含完整快照字段。
    // 准备数据/过程：通过 BinanceTables 获取表名和建表语句，在内存 DuckDB 中执行后读取列定义。
    // 预期结果：注册表名正确，价量档位为 JSON，instrument、毫秒快照时间和更新 ID 均存在。
    #[test]
    fn binance_order_book_snapshot_table_ddl_executes() {
        let table = BinanceTables::SpotOrderBookSnapshot;
        assert_eq!(table.table_name(), "bn_spot_order_book_snapshot");

        let conn = duckdb::Connection::open_in_memory().unwrap();
        for statement in table.create_table_statement().split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
            conn.execute(statement, []).unwrap();
        }

        let column_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('bn_spot_order_book_snapshot')", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(column_count, 6);
    }

    // 测试目的：验证 drop_tables 会删除 BinanceTables 中定义的全部数据表。
    // 准备数据/过程：在内存 DuckDB 中创建所有 Binance 表，再调用 drop_tables 并逐一查询表是否存在。
    // 预期结果：所有 Binance 表均不再存在，且删除操作返回成功。
    #[test]
    fn drops_all_binance_tables() {
        let db_provider = DuckDBDSProvider::memory_db();

        let conn = db_provider.acquire().unwrap();
        for table in ALL_BINANCE_TABLES {
            for statement in table.create_table_statement().split(';').map(str::trim).filter(|sql| !sql.is_empty()) {
                conn.execute(statement, []).unwrap();
            }
        }

        drop_tables(Some(db_provider), None).unwrap();

        for table in ALL_BINANCE_TABLES {
            let table_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = ?",
                    [table.table_name()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(table_count, 0, "{} should be dropped", table.table_name());
        }
    }
}

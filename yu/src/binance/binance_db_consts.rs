// 2020年1月1日零点的毫秒时间戳

pub static QUERY_LATEST_SPOT_KLINE_SQL: &str = "select symbol,max(close_time) as latest from bn_spot_kline group by symbol;";
pub static QUERY_LATEST_SWAP_KLINE_SQL: &str = "select symbol,max(close_time) as latest from bn_swap_kline group by symbol;";

pub static QUERY_LATEST_FUNDING_RATE_SQL: &str = "select symbol,max(funding_time)+60000 as latest from bn_swap_funding_rate group by symbol;";

#[derive(Clone)]
pub enum BinanceTables {
    SpotKline,
    SwapKline,
    SwapFundingRate,
    SpotTrade,
    SpotOrderEvents,
    SwapOrderEvents,
}

impl BinanceTables {
    pub fn table_name(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from("bn_spot_kline"),
            BinanceTables::SwapKline => String::from("bn_swap_kline"),
            BinanceTables::SwapFundingRate => String::from("bn_swap_funding_rate"),
            BinanceTables::SpotTrade => String::from("bn_spot_trade"),
            BinanceTables::SpotOrderEvents => String::from("bn_order_events_spot"),
            BinanceTables::SwapOrderEvents => String::from("bn_order_events_swap"),
        }
    }

    pub fn create_table_statement(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from(CREATE_SPOT_KLINE_TABLE),
            BinanceTables::SwapKline => String::from(CREATE_SWAP_KLINE_TABLE),
            BinanceTables::SwapFundingRate => String::from(CREATE_FUNDING_RATE_TABLE),
            BinanceTables::SpotTrade => String::from(BN_SPOT_TRADE_TABLE),
            BinanceTables::SpotOrderEvents => String::from(CREATE_BN_ORDER_EVENTS_SPOT_TABLE),
            BinanceTables::SwapOrderEvents => String::from(CREATE_BN_ORDER_EVENTS_SWAP_TABLE),
        }
    }

    pub fn query_lastest_record(&self) -> Option<String> {
        match self {
            BinanceTables::SpotKline => Some(String::from(QUERY_LATEST_SPOT_KLINE_SQL)),
            BinanceTables::SwapKline => Some(String::from(QUERY_LATEST_SWAP_KLINE_SQL)),
            BinanceTables::SwapFundingRate => Some(String::from(QUERY_LATEST_FUNDING_RATE_SQL)),
            _ => None,
        }
    }

    pub fn count_records(&self) -> Option<String> {
        format!("select count(*) from {}", self.table_name()).into()
    }
}

// 直接为三张表生成格式化建表SQL（不带索引），字段对齐、注释清晰
pub const CREATE_SPOT_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_spot_kline (
    id                          BIGINT,   -- 主键ID
    symbol                      VARCHAR,  -- 交易对
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_spot_kline_symbol_candle_begin_time ON bn_spot_kline(symbol, candle_begin_time);
"#;

pub const CREATE_SWAP_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_swap_kline (
    id                          BIGINT,   -- 主键ID
    symbol                      VARCHAR,  -- 交易对
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_swap_kline_symbol_candle_begin_time ON bn_swap_kline(symbol, candle_begin_time);
"#;

pub const CREATE_FUNDING_RATE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_swap_funding_rate (
    id           BIGINT,   -- 主键ID
    symbol       VARCHAR,  -- 交易对
    funding_rate DOUBLE,   -- 资金费率
    funding_time BIGINT,   -- 资金费率时间
    mark_price   DOUBLE    -- 标记价格
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_swap_funding_rate_symbol_funding_time ON bn_swap_funding_rate(symbol, funding_time);
"#;

const CREATE_BN_ORDER_EVENTS_SPOT_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_order_events_spot (
    event TEXT NOT NULL,
    account_name TEXT NOT NULL,
    event_time BIGINT NOT NULL,
    symbol TEXT NOT NULL,
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
CREATE INDEX IF NOT EXISTS idx_order_events_symbol_time ON bn_order_events_spot (symbol, event_time DESC)
"#;

const BN_SPOT_TRADE_TABLE: &str = r#"
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

// 原生方式：维护一个静态数组，便于遍历所有表类型
pub(crate) const ALL_BINANCE_TABLES: &[BinanceTables] = &[
    BinanceTables::SpotKline,
    BinanceTables::SwapKline,
    BinanceTables::SwapFundingRate,
    BinanceTables::SpotOrderEvents,
    BinanceTables::SwapOrderEvents,
    BinanceTables::SpotTrade,
];

// K线表字段定义

const CREATE_BN_ORDER_EVENTS_SWAP_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS bn_order_events_swap (
    event TEXT NOT NULL,
    account_name TEXT NOT NULL,
    event_time BIGINT NOT NULL,
    trade_time BIGINT NOT NULL,
    symbol TEXT NOT NULL,
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
CREATE INDEX IF NOT EXISTS idx_order_events_swap_symbol_time ON bn_order_events_swap (symbol, event_time DESC)
"#;

pub(crate) const ONE_HOUR_MS: u64 = 60 * 60 * 1000;

// 2020年1月1日零点的毫秒时间戳
pub(crate) const GENESIS_2020_MS: u64 = 1577836800000;

pub(super) static QUERY_LATEST_SQL: &str = "select symbol,max(candle_begin_time) as latest from spot_kline group by symbol;";

pub(crate) enum BinanceTables {
    SpotKline,
    SwapKline,
    SwapFundingRate,
}

impl BinanceTables {
    pub(crate) fn table_name(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from("spot_kline"),
            BinanceTables::SwapKline => String::from("swap_kline"),
            BinanceTables::SwapFundingRate => String::from("swap_funding_rate"),
        }
    }

    pub(crate) fn create_table_statement(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from(CREATE_SPOT_KLINE_TABLE),
            BinanceTables::SwapKline => String::from(CREATE_SWAP_KLINE_TABLE),
            BinanceTables::SwapFundingRate => String::from(CREATE_FUNDING_RATE_TABLE),
        }
    }
}

// 直接为三张表生成格式化建表SQL（不带索引），字段对齐、注释清晰
pub(crate) const CREATE_SPOT_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS spot_kline (
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
    close_time                  BIGINT    -- K线结束时间
);
"#;

pub(crate) const CREATE_SWAP_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS swap_kline (
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
    close_time                  BIGINT    -- K线结束时间
);
"#;

pub(crate) const CREATE_FUNDING_RATE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS swap_funding_rate (
    id           BIGINT,   -- 主键ID
    symbol       VARCHAR,  -- 交易对
    funding_rate DOUBLE,   -- 资金费率
    funding_time BIGINT,   -- 资金费率时间
    mark_price   DOUBLE    -- 标记价格
);
"#;

// 原生方式：维护一个静态数组，便于遍历所有表类型
pub(crate) const ALL_BINANCE_TABLES: &[BinanceTables] = &[BinanceTables::SpotKline, BinanceTables::SwapKline, BinanceTables::SwapFundingRate];

// K线表字段定义

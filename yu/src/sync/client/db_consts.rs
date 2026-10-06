use crate::postgresql_db::PostgresqlTableTrait;

#[derive(Clone)]
pub enum ClientsTables {
    PolymarketPriceHistory,
    PolyMarketInstruments,
    OkxPriceHistory,
    OkxOptionSummary,
    OkxInstruments,
    BinanceInstruments,
    BinanceSpotKline,
    BinanceSpotTrade,
}

impl PostgresqlTableTrait for ClientsTables {
    fn table_name(&self) -> &'static str {
        match self {
            ClientsTables::PolymarketPriceHistory => "polymarket_price_history",
            ClientsTables::PolyMarketInstruments => "polymarket_instruments",
            ClientsTables::OkxPriceHistory => "okx_kline_history",
            ClientsTables::OkxOptionSummary => "okx_option_summary",
            ClientsTables::OkxInstruments => "okx_instruments",
            ClientsTables::BinanceInstruments => "binance_instruments",
            ClientsTables::BinanceSpotKline => "binance_spot_kline_history",
            ClientsTables::BinanceSpotTrade => "binance_spot_trade_history",
        }
    }

    fn create_table_statement(&self) -> &'static str {
        match self {
            ClientsTables::PolymarketPriceHistory => CREATE_POLYMARKET_PRICE_HISTORY_TABLE,
            ClientsTables::PolyMarketInstruments => CREATE_POLYMARKET_INSTRUMENTS_TABLE,
            ClientsTables::OkxPriceHistory => CREATE_OKX_KLINE_HISTORY_TABLE,
            ClientsTables::OkxOptionSummary => CREATE_OKX_OPTION_SUMMARY_TABLE,
            ClientsTables::OkxInstruments => CREATE_OKX_INSTRUMENTS_TABLE,
            ClientsTables::BinanceInstruments => CREATE_BINANCE_INSTRUMENTS_TABLE,
            ClientsTables::BinanceSpotKline => CREATE_BINANCE_SPOT_KLINE_TABLE,
            ClientsTables::BinanceSpotTrade => CREATE_BINANCE_SPOT_TRADE_TABLE,
        }
    }
}

pub const CREATE_OKX_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS okx_instruments (
        id BIGINT PRIMARY KEY,
        server_id BIGINT,
        inst_identify VARCHAR,
        inst_type VARCHAR,
        inst_family VARCHAR,
        base_ccy VARCHAR,
        quote_ccy VARCHAR,
        settle_ccy VARCHAR,
        list_time BIGINT,
        exp_time BIGINT,
        tick_sz DOUBLE PRECISION,
        lot_sz DOUBLE PRECISION,
        min_sz DOUBLE PRECISION,
        alias VARCHAR,
        state VARCHAR,
        inst_id_code VARCHAR,
        inst_category VARCHAR
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_OKX_INSTRUMENTS_IDENTIFY ON okx_instruments(inst_identify);
"#;

pub const CREATE_OKX_KLINE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS okx_kline_history (
    id BIGINT,
    instrument_id BIGINT,
    timestamp TIMESTAMPTZ NOT NULL,
    open DOUBLE PRECISION,
    high DOUBLE PRECISION,
    low DOUBLE PRECISION,
    close DOUBLE PRECISION,
    vol DOUBLE PRECISION,
    vol_ccy DOUBLE PRECISION,
    vol_ccy_quote DOUBLE PRECISION,
    confirm INTEGER,
    batch_timestamp TIMESTAMPTZ
);
ALTER TABLE okx_kline_history
  ADD CONSTRAINT okx_kline_history_pkey PRIMARY KEY (instrument_id, timestamp);
SELECT create_hypertable(
  'okx_kline_history',
  'timestamp',
  if_not_exists => TRUE
);
ALTER TABLE okx_kline_history SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'timestamp DESC',
  timescaledb.segmentby = 'instrument_id'
);
SELECT add_compression_policy('okx_kline_history', INTERVAL '30 days');
"#;

pub const CREATE_OKX_OPTION_SUMMARY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS okx_option_summary (
    id BIGINT,
    instrument_id BIGINT,
    inst_identify VARCHAR,
    inst_type VARCHAR,
    uly VARCHAR,
    acquire_ts TIMESTAMPTZ,
    server_ts TIMESTAMPTZ,
    ask_vol DOUBLE PRECISION,
    bid_vol DOUBLE PRECISION,
    delta DOUBLE PRECISION,
    delta_bs DOUBLE PRECISION,
    fwd_px DOUBLE PRECISION,
    gamma DOUBLE PRECISION,
    gamma_bs DOUBLE PRECISION,
    lever DOUBLE PRECISION,
    mark_vol DOUBLE PRECISION,
    real_vol DOUBLE PRECISION,
    vol_lv DOUBLE PRECISION,
    theta DOUBLE PRECISION,
    theta_bs DOUBLE PRECISION,
    vega DOUBLE PRECISION,
    vega_bs DOUBLE PRECISION,
    batch_timestamp TIMESTAMPTZ
);
ALTER TABLE okx_option_summary
  ADD CONSTRAINT okx_option_summary_pkey PRIMARY KEY (instrument_id, acquire_ts);
SELECT create_hypertable(
  'okx_option_summary',
  'acquire_ts',
  if_not_exists => TRUE
);
ALTER TABLE okx_option_summary SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'acquire_ts DESC',
  timescaledb.segmentby = 'instrument_id'
);
SELECT add_compression_policy('okx_option_summary', INTERVAL '30 days');
"#;
pub const CREATE_POLYMARKET_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_instruments (
        id BIGINT PRIMARY KEY,
        server_id BIGINT,
        series_id VARCHAR,
        series_slug VARCHAR,
        event_id VARCHAR,
        event_slug VARCHAR,
        market_id VARCHAR,
        market_slug VARCHAR,
        assert_id VARCHAR,
        assert_slug VARCHAR,
        start_ms BIGINT,
        end_ms BIGINT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_INSTRUMENTS_TABLE_MAIN ON polymarket_instruments(assert_id);
"#;

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_price_history (
    id BIGINT,
    instrument_id BIGINT,
    timestamp TIMESTAMPTZ NOT NULL,
    price DOUBLE PRECISION,
    batch_timestamp TIMESTAMPTZ
);
ALTER TABLE polymarket_price_history
  ADD CONSTRAINT polymarket_history_pkey PRIMARY KEY (instrument_id, timestamp);
SELECT create_hypertable(
  'polymarket_price_history',
  'timestamp',
  if_not_exists => TRUE
);
ALTER TABLE polymarket_price_history SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'timestamp DESC',
  timescaledb.segmentby = 'instrument_id'
);
SELECT add_compression_policy('polymarket_price_history', INTERVAL '30 days');
"#;

pub const CREATE_BINANCE_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS binance_instruments (
    id BIGINT PRIMARY KEY,
    server_id BIGINT NOT NULL UNIQUE,
    symbol VARCHAR NOT NULL,
    status VARCHAR NOT NULL,
    base_asset VARCHAR NOT NULL,
    quote_asset VARCHAR NOT NULL,
    quote_asset_precision INTEGER NOT NULL,
    order_types TEXT[] NOT NULL,
    symbol_type VARCHAR NOT NULL,
    on_board_time BIGINT,
    UNIQUE (symbol, symbol_type)
);
"#;

pub const CREATE_BINANCE_SPOT_KLINE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS binance_spot_kline_history (
    id BIGINT NOT NULL,
    inst_id BIGINT NOT NULL,
    candle_begin_time TIMESTAMPTZ NOT NULL,
    open DOUBLE PRECISION NOT NULL,
    high DOUBLE PRECISION NOT NULL,
    low DOUBLE PRECISION NOT NULL,
    close DOUBLE PRECISION NOT NULL,
    volume DOUBLE PRECISION NOT NULL,
    quote_volume DOUBLE PRECISION NOT NULL,
    number_of_trades BIGINT NOT NULL,
    taker_buy_base_asset_volume DOUBLE PRECISION NOT NULL,
    taker_buy_quote_asset_volume DOUBLE PRECISION NOT NULL,
    close_time BIGINT NOT NULL,
    interval INTEGER NOT NULL,
    first_trade_id BIGINT,
    last_trade_id BIGINT,
    batch_timestamp BIGINT NOT NULL,
    PRIMARY KEY (inst_id,candle_begin_time)
);
SELECT create_hypertable(
  'binance_spot_kline_history',
  'candle_begin_time',
  if_not_exists => TRUE
);
ALTER TABLE binance_spot_kline_history SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'candle_begin_time DESC',
  timescaledb.segmentby = 'inst_id'
);
SELECT add_compression_policy('binance_spot_kline_history', INTERVAL '30 days');
"#;

pub const CREATE_BINANCE_SPOT_TRADE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS binance_spot_trade_history (
    id BIGINT NOT NULL,
    event_time TIMESTAMPTZ NOT NULL,
    inst_id BIGINT NOT NULL,
    trade_id BIGINT NOT NULL,
    price DOUBLE PRECISION NOT NULL,
    qty DOUBLE PRECISION NOT NULL,
    trade_time BIGINT,
    is_buyer_maker BOOLEAN,
    created_at BIGINT NOT NULL,
    batch_timestamp BIGINT NOT NULL,
    PRIMARY KEY (trade_id, event_time)
);
SELECT create_hypertable(
  'binance_spot_trade_history',
  'event_time',
  if_not_exists => TRUE
);
ALTER TABLE binance_spot_trade_history SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'event_time DESC',
  timescaledb.segmentby = 'inst_id'
);
SELECT add_compression_policy('binance_spot_trade_history', INTERVAL '30 days');
"#;

pub(crate) const ALL_CLIENT_TABLES: &[ClientsTables] = &[
    ClientsTables::PolymarketPriceHistory,
    ClientsTables::PolyMarketInstruments,
    ClientsTables::OkxPriceHistory,
    ClientsTables::OkxOptionSummary,
    ClientsTables::OkxInstruments,
    ClientsTables::BinanceInstruments,
    ClientsTables::BinanceSpotKline,
    ClientsTables::BinanceSpotTrade,
];

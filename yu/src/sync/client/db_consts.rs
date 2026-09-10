use crate::postgresql_db::PostgresqlTableTrait;

#[derive(Clone)]
pub enum ClientsTables {
    PolymarketPriceHistory,
    PolyMarketInstruments,
    OkxPriceHistory,
    OkxInstruments,
}

impl PostgresqlTableTrait for ClientsTables {
    fn table_name(&self) -> &'static str {
        match self {
            ClientsTables::PolymarketPriceHistory => "polymarket_price_history",
            ClientsTables::PolyMarketInstruments => "polymarket_instruments",
            ClientsTables::OkxPriceHistory => "okx_kline_history",
            ClientsTables::OkxInstruments => "okx_instruments",
        }
    }

    fn create_table_statement(&self) -> &'static str {
        match self {
            ClientsTables::PolymarketPriceHistory => CREATE_POLYMARKET_PRICE_HISTORY_TABLE,
            ClientsTables::PolyMarketInstruments => CREATE_POLYMARKET_INSTRUMENTS_TABLE,
            ClientsTables::OkxPriceHistory => CREATE_OKX_KLINE_HISTORY_TABLE,
            ClientsTables::OkxInstruments => CREATE_OKX_INSTRUMENTS_TABLE,
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

pub(crate) const ALL_CLIENT_TABLES: &[ClientsTables] = &[
    ClientsTables::PolymarketPriceHistory,
    ClientsTables::PolyMarketInstruments,
    ClientsTables::OkxPriceHistory,
    ClientsTables::OkxInstruments,
];

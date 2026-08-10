use crate::postgresql_db::PostgresqlTableTrait;

#[derive(Clone)]
pub enum ClientsTables {
    PriceHistory,
    Instruments,
}

impl PostgresqlTableTrait for ClientsTables {
    fn table_name(&self) -> &'static str {
        match self {
            ClientsTables::PriceHistory => "polymarket_price_history",
            ClientsTables::Instruments => "polymarket_instruments",
        }
    }

    fn create_table_statement(&self) -> &'static str {
        match self {
            ClientsTables::PriceHistory => CREATE_POLYMARKET_PRICE_HISTORY_TABLE,
            ClientsTables::Instruments => CREATE_POLYMARKET_INSTRUMENTS_TABLE,
        }
    }
}
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

pub(crate) const ALL_CLIENT_TABLES: &[ClientsTables] = &[ClientsTables::PriceHistory, ClientsTables::Instruments];

use crate::postgresql_db::PostgresqlTableTrait;

#[derive(Clone)]
pub enum PolyMarketTables {
    PriceHistory,
    AssertInfo,
}

impl PostgresqlTableTrait for PolyMarketTables {
    fn table_name(&self) -> &'static str {
        match self {
            PolyMarketTables::PriceHistory => "polymarket_price_history",
            PolyMarketTables::AssertInfo => "polymarket_assert_info",
        }
    }

    fn create_table_statement(&self) -> &'static str {
        match self {
            PolyMarketTables::PriceHistory => CREATE_POLYMARKET_PRICE_HISTORY_TABLE,
            PolyMarketTables::AssertInfo => CREATE_POLYMARKET_ASSERT_INFO_TABLE,
        }
    }
}
pub const CREATE_POLYMARKET_ASSERT_INFO_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_assert_info (
        series_id VARCHAR,
        series_slug VARCHAR,
        event_id VARCHAR,
        event_slug VARCHAR,
        market_id VARCHAR,
        market_slug VARCHAR,
        assert_id VARCHAR,
        assert_slug VARCHAR

);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_ASSERT_INFO_TABLE_MAIN ON poly_market_assert_info(assert_id);
"#;

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_price_history (
    id bigint,
    asset_id TEXT,
    timestamp timestamptz NOT NULL,
    price DOUBLE PRECISION,
    batch_timestamp bigint
);
ALTER TABLE polymarket_price_history
  ADD CONSTRAINT polymarket_history_pkey PRIMARY KEY (asset_id, timestamp);
SELECT create_hypertable(
  'polymarket_price_history',
  'timestamp',
  if_not_exists => TRUE
);
-- Enable compression and set orderby/segmentby. timescaledb.compress must be assigned a value.
ALTER TABLE polymarket_price_history SET (
  timescaledb.enable_columnstore,
  timescaledb.orderby = 'timestamp DESC',
  timescaledb.segmentby = 'asset_id'
);
SELECT add_compression_policy('polymarket_price_history', INTERVAL '30 days');
"#;

pub(crate) const ALL_CLIENT_POLYMARKET_TABLES: &[PolyMarketTables] = &[PolyMarketTables::PriceHistory, PolyMarketTables::AssertInfo];

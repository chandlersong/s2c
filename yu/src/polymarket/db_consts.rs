use crate::duck_db_tables::DuckDbTableTrait;

#[derive(Clone)]
pub enum PolyMarketTables {
    PriceHistory,
    Instruments,
}

impl DuckDbTableTrait for PolyMarketTables {
    fn table_name(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from("polymarket_price_history"),
            PolyMarketTables::Instruments => String::from("polymarket_instruments"),
        }
    }

    fn create_table_statement(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from(CREATE_POLYMARKET_PRICE_HISTORY_TABLE),
            PolyMarketTables::Instruments => String::from(CREATE_POLYMARKET_INSTRUMENTS_TABLE),
        }
    }

    fn query_lastest_record(&self) -> Option<String> {
        todo!()
    }
}
pub const CREATE_POLYMARKET_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_instruments (
        id BIGINT,
        series_id VARCHAR,
        series_slug VARCHAR,
        event_id VARCHAR,
        event_slug VARCHAR,
        market_id VARCHAR,
        market_slug VARCHAR,
        asset_id VARCHAR,
        asset_slug VARCHAR,
        start_ms BIGINT,
        end_ms BIGINT,
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_INSTRUMENTS_TABLE_MAIN ON polymarket_instruments(asset_id);
"#;

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_price_history (
        instrument_id BIGINT,
        timestamp BIGINT,
        price DOUBLE
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_PRICE_HISTORY_TABLE_MAIN ON polymarket_price_history(instrument_id, timestamp);
"#;

pub(crate) const ALL_POLYMARKET_TABLES: &[PolyMarketTables] = &[PolyMarketTables::PriceHistory, PolyMarketTables::Instruments];

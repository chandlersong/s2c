use crate::duck_db_tables::DuckDbTableTrait;

#[derive(Clone)]
pub enum PolyMarketTables {
    PriceHistory,
    AssertInfo,
}

impl DuckDbTableTrait for PolyMarketTables {
    fn table_name(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from("polymarket_price_history"),
            PolyMarketTables::AssertInfo => String::from("polymarket_assert_info"),
        }
    }

    fn create_table_statement(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from(CREATE_POLYMARKET_PRICE_HISTORY_TABLE),
            PolyMarketTables::AssertInfo => String::from(CREATE_POLYMARKET_ASSERT_INFO_TABLE),
        }
    }

    fn query_lastest_record(&self) -> Option<String> {
        todo!()
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
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_ASSERT_INFO_TABLE_MAIN ON polymarket_assert_info(assert_id);
"#;

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS polymarket_price_history (
        assert_id VARCHAR,
        timestamp BIGINT,
        price DOUBLE
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_PRICE_HISTORY_TABLE_MAIN ON polymarket_price_history(assert_id, timestamp);
"#;

pub(crate) const ALL_POLYMARKET_TABLES: &[PolyMarketTables] = &[PolyMarketTables::PriceHistory, PolyMarketTables::AssertInfo];

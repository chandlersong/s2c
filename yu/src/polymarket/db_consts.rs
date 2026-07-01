use crate::duck_db_tables::DuckDbTableTrait;

#[derive(Clone)]
pub enum PolyMarketTables {
    PriceHistory,
}

impl DuckDbTableTrait for PolyMarketTables {
    fn table_name(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from("poly_market_price_history"),
        }
    }

    fn create_table_statement(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from(CREATE_POLYMARKET_PRICE_HISTORY_TABLE),
        }
    }

    fn query_lastest_record(&self) -> Option<String> {
        todo!()
    }
}

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS poly_market_price_history (
        assert_id VARCHAR,
        timestamp BIGINT,
        payload BLOB
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_PRICE_HISTORY_TABLE_MAIN ON poly_market_price_history(assert_id, timestamp);
"#;

pub(crate) const ALL_POLYMARKET_TABLES: &[PolyMarketTables] = &[PolyMarketTables::PriceHistory];

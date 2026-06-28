use crate::binance::binance_db_consts::BinanceTables;

#[derive(Clone)]
pub enum PolyMarketTables {
    PriceHistory,
}

impl PolyMarketTables {
    pub fn table_name(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from("poly_market_price_history"),
        }
    }

    pub fn create_table_statement(&self) -> String {
        match self {
            PolyMarketTables::PriceHistory => String::from(CREATE_POLYMARKET_PRICE_HISTORY_TABLE),
        }
    }

    pub fn count_records(&self) -> Option<String> {
        format!("select count(*) from {}", self.table_name()).into()
    }
}

pub const CREATE_POLYMARKET_PRICE_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS poly_market_price_history (
        assert_id VARCHAR，
        timestamp BIGINT
        payload BLOB
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_POLYMARKET_PRICE_HISTORY_TABLE_MAIN ON poly_market_price_history(assert_id, timestamp);
"#;

pub(crate) const ALL_POLYMARKET_TABLES: &[PolyMarketTables] = &[PolyMarketTables::PriceHistory];

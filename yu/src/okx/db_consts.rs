use crate::duck_db_tables::DuckDbTableTrait;

#[derive(Clone)]
pub enum OkxTables {
    Kline,
    Instruments,
}

impl DuckDbTableTrait for OkxTables {
    fn table_name(&self) -> String {
        match self {
            OkxTables::Kline => String::from("OKX_KLINE"),
            OkxTables::Instruments => String::from("OKX_INSTRUMENTS"),
        }
    }

    fn create_table_statement(&self) -> String {
        match self {
            OkxTables::Kline => String::from(OKX_KLINE),
            OkxTables::Instruments => String::from(CREATE_OKX_INSTRUMENTS_TABLE),
        }
    }

    fn query_lastest_record(&self) -> Option<String> {
        todo!()
    }
}
pub const CREATE_OKX_INSTRUMENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS okx_instruments (
        instId VARCHAR,
        instType VARCHAR,
        instFamily VARCHAR,
        baseCcy DOUBLE,
        quoteCcy DOUBLE,
        settleCcy DOUBLE,
        listTime VARCHAR,
        expTime VARCHAR,
        tickSz DOUBLE,
        lotSz DOUBLE,
        minSz DOUBLE,
        alias VARCHAR,
        state VARCHAR,
        instIdCode VARCHAR,
        instCategory VARCHAR
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_okx_instruments_MAIN ON okx_instruments(instId);
"#;

pub const OKX_KLINE: &str = r#"
CREATE TABLE IF NOT EXISTS OKX_KLINE (
        id BIGINT PRIMARY KEY,
        instId VARCHAR,
        timestamp BIGINT,
        open DOUBLE,
        high DOUBLE,
        low DOUBLE,
        close DOUBLE,
        volume DOUBLE,
        volCcy DOUBLE,
        volCcyQuote DOUBLE,
        confirm int
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_CREATE_OKX_KLINE_MAIN ON OKX_KLINE(instId, timestamp);
"#;

pub(crate) const ALL_OKX_TABLES: &[OkxTables] = &[OkxTables::Kline, OkxTables::Instruments];

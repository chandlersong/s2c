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
CREATE TABLE IF NOT EXISTS OKX_INSTRUMENTS (
        id BIGINT PRIMARY KEY,
        inst_identify VARCHAR,
        inst_type VARCHAR,
        inst_family VARCHAR,
        base_ccy VARCHAR,
        quote_ccy VARCHAR,
        settle_ccy VARCHAR,
        list_time BIGINT,
        exp_time BIGINT,
        tick_sz DOUBLE,
        lot_sz DOUBLE,
        min_sz DOUBLE,
        alias VARCHAR,
        state VARCHAR,
        inst_id_code VARCHAR,
        inst_category VARCHAR
);
"#;

pub const OKX_KLINE: &str = r#"
CREATE TABLE IF NOT EXISTS OKX_KLINE (
        id BIGINT PRIMARY KEY,
        inst_id BIGINT,
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
CREATE UNIQUE INDEX IF NOT EXISTS IDX_CREATE_OKX_KLINE_MAIN ON OKX_KLINE(inst_id, timestamp);
"#;

pub(crate) const ALL_OKX_TABLES: &[OkxTables] = &[OkxTables::Kline, OkxTables::Instruments];

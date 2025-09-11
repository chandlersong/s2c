pub(crate) const ONE_HOUR_MS: u64 = 60 * 60 * 1000;

pub(super) static QUERY_LATEST_SQL: &str =
    "select symbol,max(candle_begin_time) as latest from spot_kline group by symbol;";

static CREATE_KLINE_TABLE: &str = "CREATE TABLE IF NOT EXISTS spot_kline
                        (
                            id BIGINT,
                            symbol VARCHAR,
                            candle_begin_time BIGINT,
                            open DOUBLE,
                            high DOUBLE,
                            low DOUBLE,
                            close DOUBLE,
                            volume DOUBLE,
                            quote_volume DOUBLE,
                            number_of_trades BIGINT,
                            taker_buy_base_asset_volume DOUBLE,
                            taker_buy_quote_asset_volume DOUBLE,
                            close_time BIGINT
                        );";

pub(crate) enum BinanceTables {
    SpotKline,
}

impl BinanceTables {
    pub(crate) fn table_name(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from("spot_kline"),
        }
    }

    pub(crate) fn create_table_statement(&self) -> String {
        match self {
            BinanceTables::SpotKline => String::from(CREATE_KLINE_TABLE),
        }
    }
}

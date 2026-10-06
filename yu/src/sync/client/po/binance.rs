use crate::postgresql_db::CopyInsertable;
use crate::sync::models::grpc_sync::{BinanceInstrument, BinanceKline, BinanceTrade};
use li::tools::time::format_timestamp_millis;
use sqlx::Row;
use yue::tools::get_snow_flake_id_u64;

#[derive(Clone, Debug)]
pub struct LocalBinanceInstrumentPo {
    pub id: u64,
    pub server_id: u64,
    pub symbol: String,
    pub status: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub quote_asset_precision: i32,
    pub order_types: Vec<String>,
    pub symbol_type: String,
    pub on_board_time: Option<u64>,
}

impl LocalBinanceInstrumentPo {
    pub fn from_proto(instrument: BinanceInstrument) -> Self {
        Self {
            id: get_snow_flake_id_u64(),
            server_id: instrument.server_id,
            symbol: instrument.symbol,
            status: instrument.status,
            base_asset: instrument.base_asset,
            quote_asset: instrument.quote_asset,
            quote_asset_precision: instrument.quote_asset_precision,
            order_types: instrument.order_types,
            symbol_type: instrument.symbol_type,
            on_board_time: instrument.on_board_time,
        }
    }
}

impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for LocalBinanceInstrumentPo {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get::<i64, _>("id")? as u64,
            server_id: row.try_get::<i64, _>("server_id")? as u64,
            symbol: row.try_get("symbol")?,
            status: row.try_get("status")?,
            base_asset: row.try_get("base_asset")?,
            quote_asset: row.try_get("quote_asset")?,
            quote_asset_precision: row.try_get("quote_asset_precision")?,
            order_types: row.try_get("order_types")?,
            symbol_type: row.try_get("symbol_type")?,
            on_board_time: row.try_get::<Option<i64>, _>("on_board_time")?.map(|value| value as u64),
        })
    }
}

#[derive(Clone, Debug)]
pub struct LocalBinanceKlinePo {
    pub id: u64,
    pub symbol: String,
    pub candle_begin_time: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub quote_volume: f64,
    pub number_of_trades: u64,
    pub taker_buy_base_asset_volume: f64,
    pub taker_buy_quote_asset_volume: f64,
    pub close_time: u64,
    pub interval: u32,
    pub first_trade_id: Option<i64>,
    pub last_trade_id: Option<i64>,
    pub batch_timestamp: u64,
}

impl LocalBinanceKlinePo {
    pub fn from_proto(kline: BinanceKline, batch_timestamp: u64) -> Self {
        Self {
            id: get_snow_flake_id_u64(),
            symbol: kline.symbol,
            candle_begin_time: kline.candle_begin_time,
            open: kline.open,
            high: kline.high,
            low: kline.low,
            close: kline.close,
            volume: kline.volume,
            quote_volume: kline.quote_volume,
            number_of_trades: kline.number_of_trades,
            taker_buy_base_asset_volume: kline.taker_buy_base_asset_volume,
            taker_buy_quote_asset_volume: kline.taker_buy_quote_asset_volume,
            close_time: kline.close_time,
            interval: kline.interval,
            first_trade_id: kline.first_trade_id,
            last_trade_id: kline.last_trade_id,
            batch_timestamp,
        }
    }
}

impl CopyInsertable for LocalBinanceKlinePo {
    fn columns() -> &'static str {
        "id,symbol,candle_begin_time,open,high,low,close,volume,quote_volume,number_of_trades,taker_buy_base_asset_volume,taker_buy_quote_asset_volume,close_time,interval,first_trade_id,last_trade_id,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        let candle_begin_time =
            format_timestamp_millis(i64::try_from(self.candle_begin_time).expect("candle_begin_time must fit in i64 milliseconds"));

        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.id,
            self.symbol,
            candle_begin_time,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.quote_volume,
            self.number_of_trades,
            self.taker_buy_base_asset_volume,
            self.taker_buy_quote_asset_volume,
            self.close_time,
            self.interval,
            self.first_trade_id.map_or(String::new(), |value| value.to_string()),
            self.last_trade_id.map_or(String::new(), |value| value.to_string()),
            self.batch_timestamp
        )
    }
}

#[derive(Clone, Debug)]
pub struct LocalBinanceTradePo {
    pub id: u64,
    pub event_time: i64,
    pub symbol: String,
    pub trade_id: i64,
    pub price: f64,
    pub qty: f64,
    pub trade_time: Option<i64>,
    pub is_buyer_maker: Option<bool>,
    pub created_at: i64,
    pub batch_timestamp: u64,
}

impl LocalBinanceTradePo {
    pub fn from_proto(trade: BinanceTrade, batch_timestamp: u64) -> Self {
        Self {
            id: get_snow_flake_id_u64(),
            event_time: trade.event_time,
            symbol: trade.symbol,
            trade_id: trade.trade_id,
            price: trade.price,
            qty: trade.qty,
            trade_time: trade.trade_time,
            is_buyer_maker: trade.is_buyer_maker,
            created_at: trade.created_at,
            batch_timestamp,
        }
    }
}

impl CopyInsertable for LocalBinanceTradePo {
    fn columns() -> &'static str {
        "id,event_time,symbol,trade_id,price,qty,trade_time,is_buyer_maker,created_at,batch_timestamp"
    }

    fn to_csv_row(&self) -> String {
        let event_time = format_timestamp_millis(self.event_time);

        format!(
            "{},{},{},{},{},{},{},{},{},{}",
            self.id,
            event_time,
            self.symbol,
            self.trade_id,
            self.price,
            self.qty,
            self.trade_time.map_or(String::new(), |value| value.to_string()),
            self.is_buyer_maker.map_or(String::new(), |value| value.to_string()),
            self.created_at,
            self.batch_timestamp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binance_instrument_proto_maps_all_fields_to_client_po() {
        let proto = BinanceInstrument {
            server_id: 17,
            symbol: "BTCUSDT".to_string(),
            status: "TRADING".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string(), "MARKET".to_string()],
            symbol_type: "PERPETUAL".to_string(),
            on_board_time: Some(1_600_000_000_000),
        };

        let po = LocalBinanceInstrumentPo::from_proto(proto);

        assert_eq!(po.server_id, 17);
        assert_eq!(po.symbol, "BTCUSDT");
        assert_eq!(po.status, "TRADING");
        assert_eq!(po.base_asset, "BTC");
        assert_eq!(po.quote_asset, "USDT");
        assert_eq!(po.quote_asset_precision, 8);
        assert_eq!(po.order_types, vec!["LIMIT", "MARKET"]);
        assert_eq!(po.symbol_type, "PERPETUAL");
        assert_eq!(po.on_board_time, Some(1_600_000_000_000));
    }

    #[test]
    fn binance_kline_proto_maps_to_client_po() {
        let proto = BinanceKline {
            id: 10,
            symbol: "BTCUSDT".to_string(),
            candle_begin_time: 1_700_000_000_000,
            open: 100.25,
            high: 110.5,
            low: 95.0,
            close: 105.75,
            volume: 12.5,
            quote_volume: 1_200.0,
            number_of_trades: 11,
            taker_buy_base_asset_volume: 7.0,
            taker_buy_quote_asset_volume: 700.0,
            close_time: 1_700_000_299_999,
            interval: 1,
            first_trade_id: Some(20),
            last_trade_id: Some(30),
        };

        let po = LocalBinanceKlinePo::from_proto(proto, 1_700_000_300_000);

        assert_eq!(po.symbol, "BTCUSDT");
        assert_eq!(po.candle_begin_time, 1_700_000_000_000);
        assert_eq!(po.first_trade_id, Some(20));
        assert_eq!(po.last_trade_id, Some(30));
        assert_eq!(po.batch_timestamp, 1_700_000_300_000);
    }

    #[test]
    fn binance_trade_proto_preserves_optional_fields() {
        let proto = BinanceTrade {
            id: 10,
            event_time: 1_700_000_000_001,
            symbol: "BTCUSDT".to_string(),
            trade_id: 42,
            price: 100.25,
            qty: 0.5,
            trade_time: None,
            is_buyer_maker: Some(true),
            created_at: 0,
        };

        let po = LocalBinanceTradePo::from_proto(proto, 1_700_000_000_002);

        assert_eq!(po.symbol, "BTCUSDT");
        assert_eq!(po.trade_time, None);
        assert_eq!(po.is_buyer_maker, Some(true));
        assert_eq!(po.batch_timestamp, 1_700_000_000_002);
    }

    #[test]
    fn binance_kline_copy_row_formats_candle_time_as_timestamp() {
        let proto = BinanceKline {
            id: 10,
            symbol: "BTCUSDT".to_string(),
            candle_begin_time: 1_700_000_000_000,
            open: 100.25,
            high: 110.5,
            low: 95.0,
            close: 105.75,
            volume: 12.5,
            quote_volume: 1_200.0,
            number_of_trades: 11,
            taker_buy_base_asset_volume: 7.0,
            taker_buy_quote_asset_volume: 700.0,
            close_time: 1_700_000_299_999,
            interval: 1,
            first_trade_id: None,
            last_trade_id: None,
        };
        let po = LocalBinanceKlinePo::from_proto(proto, 1_700_000_300_000);

        assert!(po.to_csv_row().contains("2023-11-14T22:13:20+00:00"));
    }

    #[test]
    fn binance_trade_copy_row_formats_event_time_as_timestamp() {
        let proto = BinanceTrade {
            id: 10,
            event_time: 1_700_000_000_001,
            symbol: "BTCUSDT".to_string(),
            trade_id: 42,
            price: 100.25,
            qty: 0.5,
            trade_time: None,
            is_buyer_maker: Some(true),
            created_at: 0,
        };
        let po = LocalBinanceTradePo::from_proto(proto, 1_700_000_000_002);

        assert!(po.to_csv_row().contains("2023-11-14T22:13:20.001+00:00"));
    }
}

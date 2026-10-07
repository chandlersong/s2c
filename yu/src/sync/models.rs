pub mod grpc_sync {
    use crate::polymarket::po::PolyMarketInstrumentPo;
    use li::tools::time::unix_time_now_u64_utc;

    tonic::include_proto!("grpc_sync");

    impl From<PolyMarketInstrumentPo> for PolymarketInstrument {
        fn from(value: PolyMarketInstrumentPo) -> Self {
            PolymarketInstrument {
                server_id: value.id,
                series_id: value.series_id,
                series_slug: value.series_slug,
                event_id: value.event_id,
                event_slug: value.event_slug,
                market_id: value.market_id,
                market_slug: value.market_slug,
                asset_id: value.asset_id.clone(),
                asset_slug: value.asset_slug,
                start_ms: value.start_ms,
                end_ms: value.end_ms,
                latest_timestamp: unix_time_now_u64_utc(),
            }
        }
    }

    impl From<crate::binance::models::po::BinanceInstrument> for BinanceInstrument {
        fn from(value: crate::binance::models::po::BinanceInstrument) -> Self {
            Self {
                server_id: value.id as u64,
                symbol: value.symbol,
                status: value.status,
                base_asset: value.base_asset,
                quote_asset: value.quote_asset,
                quote_asset_precision: value.quote_asset_precision,
                order_types: value.order_types,
                symbol_type: value.symbol_type.to_string(),
                on_board_time: value.on_board_time,
            }
        }
    }

    // Map okx InstrumentPo to proto OkxInstrument
    impl From<crate::okx::duck_po::InstrumentPo> for OkxInstrument {
        fn from(value: crate::okx::duck_po::InstrumentPo) -> Self {
            OkxInstrument {
                server_id: value.id,
                inst_id: value.inst_identify,
                inst_type: value.inst_type,
                inst_family: value.inst_family.unwrap_or_default(),
                base_ccy: value.base_ccy,
                quote_ccy: value.quote_ccy.unwrap_or_default(),
                settle_ccy: value.settle_ccy.unwrap_or_default(),
                list_time: value.list_time.unwrap_or(0),
                exp_time: value.exp_time.unwrap_or(0),
                tick_sz: value.tick_sz.unwrap_or(0.0),
                lot_sz: value.lot_sz.unwrap_or(0.0),
                min_sz: value.min_sz.unwrap_or(0.0),
                alias: value.alias.unwrap_or_default(),
                state: value.state.unwrap_or_default(),
                inst_id_code: value.inst_id_code.unwrap_or_default(),
                inst_category: value.inst_category.unwrap_or_default(),
            }
        }
    }

    impl From<crate::binance::models::po::KlinePo> for BinanceKline {
        fn from(value: crate::binance::models::po::KlinePo) -> Self {
            BinanceKline {
                id: value.id as u64,
                symbol: value.symbol,
                candle_begin_time: value.candle_begin_time,
                open: value.open,
                high: value.high,
                low: value.low,
                close: value.close,
                volume: value.volume,
                quote_volume: value.quote_volume,
                number_of_trades: value.number_of_trades,
                taker_buy_base_asset_volume: value.taker_buy_base_asset_volume,
                taker_buy_quote_asset_volume: value.taker_buy_quote_asset_volume,
                close_time: value.close_time,
                interval: value.interval.into(),
                first_trade_id: value.first_trade_id,
                last_trade_id: value.last_trade_id,
            }
        }
    }

    impl From<crate::binance::models::SpotStreamTradeRecordPo> for BinanceTrade {
        fn from(value: crate::binance::models::SpotStreamTradeRecordPo) -> Self {
            BinanceTrade {
                id: value.id as u64,
                event_time: value.event_time,
                symbol: value.symbol,
                trade_id: value.trade_id,
                price: value.price,
                qty: value.qty,
                trade_time: value.trade_time,
                is_buyer_maker: value.is_buyer_maker,
                created_at: value.created_at,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grpc_sync::{BinanceKline, BinanceTrade};
    use crate::binance::models::po::{BinanceInstrument as BinanceInstrumentPo, KlinePo, SpotStreamTradeRecordPo};
    use crate::sync::client::po::binance::{LocalBinanceKlinePo, LocalBinanceTradePo};
    use yue::models::InstrumentType;

    #[test]
    fn binance_instrument_server_to_proto_preserves_fields() {
        let server_po = BinanceInstrumentPo {
            id: 7,
            symbol: "BTCUSDT".to_string(),
            status: "TRADING".to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            symbol_type: InstrumentType::Swap,
            on_board_time: Some(1_600_000_000_000),
        };

        let proto = super::grpc_sync::BinanceInstrument::from(server_po);

        assert_eq!(proto.server_id, 7);
        assert_eq!(proto.symbol, "BTCUSDT");
        assert_eq!(proto.status, "TRADING");
        assert_eq!(proto.base_asset, "BTC");
        assert_eq!(proto.quote_asset, "USDT");
        assert_eq!(proto.quote_asset_precision, 8);
        assert_eq!(proto.order_types, vec!["LIMIT"]);
        assert_eq!(proto.symbol_type, "SWAP");
        assert_eq!(proto.on_board_time, Some(1_600_000_000_000));
    }

    #[test]
    fn binance_kline_server_to_client_mapping_preserves_fields() {
        let server_po = KlinePo {
            id: 7,
            symbol: "BTCUSDT".to_string(),
            candle_begin_time: 1_700_000_000_000,
            open: 100.0,
            high: 110.0,
            low: 90.0,
            close: 105.0,
            volume: 2.0,
            quote_volume: 210.0,
            number_of_trades: 20,
            taker_buy_base_asset_volume: 1.0,
            taker_buy_quote_asset_volume: 105.0,
            close_time: 1_700_000_299_999,
            interval: 1,
            first_trade_id: Some(3),
            last_trade_id: Some(22),
        };

        let proto = BinanceKline::from(server_po);
        let client_po = LocalBinanceKlinePo::from_proto(proto, 1_700_000_300_000);

        assert_eq!(client_po.symbol, "BTCUSDT");
        assert_eq!(client_po.candle_begin_time, 1_700_000_000_000);
        assert_eq!(client_po.first_trade_id, Some(3));
    }

    #[test]
    fn binance_trade_server_to_client_mapping_preserves_nullable_fields() {
        let server_po = SpotStreamTradeRecordPo {
            id: 9,
            event_time: 1_700_000_000_001,
            symbol: "BTCUSDT".to_string(),
            trade_id: 42,
            price: 100.25,
            qty: 0.5,
            trade_time: None,
            is_buyer_maker: Some(true),
            created_at: 0,
        };

        let proto = BinanceTrade::from(server_po);
        let client_po = LocalBinanceTradePo::from_proto(proto, 1_700_000_000_002);

        assert_eq!(client_po.trade_id, 42);
        assert_eq!(client_po.trade_time, None);
        assert_eq!(client_po.is_buyer_maker, Some(true));
    }
}

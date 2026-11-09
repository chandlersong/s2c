pub mod grpc_sync {
    use crate::polymarket::po::PolyMarketInstrumentPo;
    use li::tools::time::unix_time_now_u64_utc;
    use tokio::sync::mpsc;

    tonic::include_proto!("grpc_sync");

    pub type ClientBatchWriter = mpsc::Sender<ServerMessage>;
    impl InstrumentType {
        pub fn readable_name(self) -> &'static str {
            match self {
                Self::Polymarket => "Polymarket",
                Self::Okx => "OKX",
                Self::Binance => "Binance",
                Self::BinanceOption => "Binance Option",
                InstrumentType::ExchangeUnspecified => "ExchangeUnspecified",
            }
        }
    }

    impl HistoryType {
        pub fn readable_name(self) -> &'static str {
            match self {
                Self::PolymarketToken => "Polymarket Token",
                Self::OkxKline => "OKX Kline",
                Self::OkxOptionSummary => "OKX Option Summary",
                Self::BinanceSpot => "Binance Spot",
                Self::BinanceTrade => "Binance Trade",
                Self::InstrumentUnspecified => "InstrumentUnspecified",
                Self::BinanceSwap => "Binance Swap",
                Self::BinanceSwapTrade => "Binance Swap Trade",
                Self::BinanceSwapFundingRate => "Binance Swap Funding Rate",
                Self::BinanceOrderBookSnapshot => "Binance Order Book Snapshot",
            }
        }
    }

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

    impl TryFrom<crate::binance::models::po::BinanceOptionInstrumentPo> for BinanceOptionInstrument {
        type Error = std::num::TryFromIntError;

        fn try_from(value: crate::binance::models::po::BinanceOptionInstrumentPo) -> Result<Self, Self::Error> {
            Ok(Self {
                server_id: u64::try_from(value.id)?,
                expiry_date: value.expiry_date,
                symbol: value.symbol,
                side: value.side,
                strike_price: value.strike_price.to_string(),
                underlying: value.underlying,
                unit: value.unit,
                liquidation_fee_rate: value.liquidation_fee_rate.to_string(),
                min_qty: value.min_qty.to_string(),
                max_qty: value.max_qty.to_string(),
                initial_margin: value.initial_margin.to_string(),
                maintenance_margin: value.maintenance_margin.to_string(),
                min_initial_margin: value.min_initial_margin.to_string(),
                min_maintenance_margin: value.min_maintenance_margin.to_string(),
                price_scale: value.price_scale,
                quantity_scale: value.quantity_scale,
                quote_asset: value.quote_asset,
                contract_type: value.contract_type,
                underlying_type: value.underlying_type,
                naked_sell: value.naked_sell,
                status: value.status,
                filters: value
                    .filters
                    .into_iter()
                    .map(|filter| BinanceOptionSymbolFilter {
                        filter_type: filter.filter_type,
                        min_price: filter.min_price.map(|value| value.to_string()),
                        max_price: filter.max_price.map(|value| value.to_string()),
                        tick_size: filter.tick_size.map(|value| value.to_string()),
                        min_qty: filter.min_qty.map(|value| value.to_string()),
                        max_qty: filter.max_qty.map(|value| value.to_string()),
                        step_size: filter.step_size.map(|value| value.to_string()),
                    })
                    .collect(),
            })
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
                trade_id: value.trade_id,
                price: value.price,
                qty: value.qty,
                trade_time: value.trade_time,
                is_buyer_maker: value.is_buyer_maker,
                created_at: value.created_at,
            }
        }
    }

    impl From<crate::binance::models::po::FundingRatePo> for BinanceFundingRate {
        fn from(value: crate::binance::models::po::FundingRatePo) -> Self {
            Self {
                id: value.id as u64,
                funding_time: value.funding_time,
                funding_rate: value.funding_rate,
                mark_price: value.mark_price,
            }
        }
    }

    impl From<crate::binance::models::po::BinanceOrderBookSnapshotPo> for BinanceOrderBookSnapshot {
        fn from(value: crate::binance::models::po::BinanceOrderBookSnapshotPo) -> Self {
            Self {
                server_id: value.id as u64,
                snapshot_time: value.snapshot_time,
                local_update_id: value.local_update_id,
                bids: value
                    .bids
                    .into_iter()
                    .map(|level| BinanceOrderBookLevel {
                        price: level.price,
                        quantity: level.quantity,
                    })
                    .collect(),
                asks: value
                    .asks
                    .into_iter()
                    .map(|level| BinanceOrderBookLevel {
                        price: level.price,
                        quantity: level.quantity,
                    })
                    .collect(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::grpc_sync::{BinanceFundingRate, BinanceKline, BinanceTrade, HistoryType, InstrumentType as GrpcInstrumentType, instrument};
    use crate::binance::models::po::BinanceOrderBookSnapshotPo;
    use crate::binance::models::po::{BinanceInstrument as BinanceInstrumentPo, KlinePo, SpotStreamTradeRecordPo};
    use crate::sync::client::po::binance::LocalBinanceOrderBookSnapshotPo;
    use crate::sync::client::po::binance::{LocalBinanceKlinePo, LocalBinanceOptionInstrumentPo, LocalBinanceTradePo};
    use rust_decimal::Decimal;
    use yue::binance::bn_models::option_restful::{OptionSymbol, OptionSymbolFilter};
    use yue::binance::bn_models::spot_restful::Depth;
    use yue::binance::order_book::OrderBook;
    use yue::models::InstrumentType;

    #[test]
    fn exchange_type_has_readable_name() {
        assert_eq!(GrpcInstrumentType::Polymarket.readable_name(), "Polymarket");
        assert_eq!(GrpcInstrumentType::Okx.readable_name(), "OKX");
        assert_eq!(GrpcInstrumentType::Binance.readable_name(), "Binance");
    }

    // 测试目的：验证新增 Binance Option instrument 类型的可读名称。
    // 准备数据/过程：调用 protobuf InstrumentType 的 readable_name 方法。
    // 预期结果：BinanceOption 返回 "Binance Option"。
    #[test]
    fn binance_option_instrument_type_has_readable_name() {
        assert_eq!(GrpcInstrumentType::BinanceOption.readable_name(), "Binance Option");
    }

    #[test]
    fn instrument_type_has_readable_name() {
        assert_eq!(HistoryType::PolymarketToken.readable_name(), "Polymarket Token");
        assert_eq!(HistoryType::OkxKline.readable_name(), "OKX Kline");
        assert_eq!(HistoryType::OkxOptionSummary.readable_name(), "OKX Option Summary");
        assert_eq!(HistoryType::BinanceSpot.readable_name(), "Binance Spot");
        assert_eq!(HistoryType::BinanceTrade.readable_name(), "Binance Trade");
        assert_eq!(HistoryType::InstrumentUnspecified.readable_name(), "InstrumentUnspecified");
    }

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

    // 测试目的：验证 Binance OptionSymbol 从 VO 经服务端 PO 和 protobuf 到客户端 PO 的字段与 ID 映射。
    // 准备数据/过程：构造包含精确 Decimal、nullable filter 和 naked_sell 的 VO，以固定服务器 ID 转换并创建客户端 PO。
    // 预期结果：毫秒到期时间、Decimal 字符串、nullable 字段与 filters 完整保留；server_id 保持原值，客户端本地 id 独立生成，负服务器 ID 转换失败。
    #[test]
    fn binance_option_symbol_maps_vo_server_po_proto_and_client_po() {
        let source = OptionSymbol {
            expiry_date: 1_700_000_000_123,
            filters: vec![OptionSymbolFilter {
                filter_type: "PRICE_FILTER".to_string(),
                min_price: Some("0.00000001".parse().unwrap()),
                max_price: Some("80000.01000000".parse().unwrap()),
                tick_size: Some("0.01000000".parse().unwrap()),
                min_qty: Some("0.01".parse().unwrap()),
                max_qty: None,
                step_size: Some("0.001".parse().unwrap()),
            }],
            symbol: "BTC-270101-50000-C".to_string(),
            side: "CALL".to_string(),
            strike_price: "50000.00000001".parse().unwrap(),
            underlying: "BTCUSDT".to_string(),
            unit: 1,
            liquidation_fee_rate: "0.0019000".parse().unwrap(),
            min_qty: "0.01".parse().unwrap(),
            max_qty: "100".parse().unwrap(),
            initial_margin: "0.15".parse().unwrap(),
            maintenance_margin: "0.075".parse().unwrap(),
            min_initial_margin: "0.1".parse().unwrap(),
            min_maintenance_margin: "0.05".parse().unwrap(),
            price_scale: 2,
            quantity_scale: 2,
            quote_asset: "USDT".to_string(),
            contract_type: "CRYPTO_OPTIONS".to_string(),
            underlying_type: "CRYPTO".to_string(),
            naked_sell: Some(false),
            status: "TRADING".to_string(),
        };
        let server_po = crate::binance::models::po::BinanceOptionInstrumentPo::from_option_symbol_with_id(&source, 123);
        let instrument_message = server_po.into_proto_instrument().unwrap();
        assert_eq!(instrument_message.instrument_type, GrpcInstrumentType::BinanceOption as i32);
        let Some(instrument::Payload::BinanceOptionInstrument(proto)) = instrument_message.payload else {
            panic!("instrument payload is not BinanceOptionInstrument");
        };
        let client_po = LocalBinanceOptionInstrumentPo::from_proto(proto);
        let invalid_id_po = crate::binance::models::po::BinanceOptionInstrumentPo::from_option_symbol_with_id(&source, -1);
        assert!(invalid_id_po.into_proto_instrument().is_err());

        assert_eq!(client_po.server_id, 123);
        assert_ne!(client_po.id, client_po.server_id);
        assert_eq!(client_po.expiry_date, 1_700_000_000_123);
        assert_eq!(client_po.symbol, "BTC-270101-50000-C");
        assert_eq!(client_po.side, "CALL");
        assert_eq!(client_po.strike_price, "50000.00000001");
        assert_eq!(client_po.underlying, "BTCUSDT");
        assert_eq!(client_po.unit, 1);
        assert_eq!(client_po.liquidation_fee_rate, "0.0019000");
        assert_eq!(client_po.min_qty, "0.01");
        assert_eq!(client_po.max_qty, "100");
        assert_eq!(client_po.initial_margin, "0.15");
        assert_eq!(client_po.maintenance_margin, "0.075");
        assert_eq!(client_po.min_initial_margin, "0.1");
        assert_eq!(client_po.min_maintenance_margin, "0.05");
        assert_eq!(client_po.price_scale, 2);
        assert_eq!(client_po.quantity_scale, 2);
        assert_eq!(client_po.quote_asset, "USDT");
        assert_eq!(client_po.contract_type, "CRYPTO_OPTIONS");
        assert_eq!(client_po.underlying_type, "CRYPTO");
        assert_eq!(client_po.naked_sell, Some(false));
        assert_eq!(client_po.status, "TRADING");
        assert_eq!(client_po.filters.len(), 1);
        assert_eq!(client_po.filters[0].filter_type, "PRICE_FILTER");
        assert_eq!(client_po.filters[0].min_price.as_deref(), Some("0.00000001"));
        assert_eq!(client_po.filters[0].max_price.as_deref(), Some("80000.01000000"));
        assert_eq!(client_po.filters[0].tick_size.as_deref(), Some("0.01000000"));
        assert_eq!(client_po.filters[0].min_qty.as_deref(), Some("0.01"));
        assert_eq!(client_po.filters[0].max_qty, None);
        assert_eq!(client_po.filters[0].step_size.as_deref(), Some("0.001"));
    }

    #[test]
    fn binance_kline_server_to_client_mapping_preserves_fields() {
        let server_po = KlinePo {
            id: 7,
            inst_id: 123,
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
        let client_po = LocalBinanceKlinePo::from_proto(proto, 7, 1_700_000_300_000);

        assert_eq!(client_po.candle_begin_time, 1_700_000_000_000);
        assert_eq!(client_po.first_trade_id, Some(3));
    }

    #[test]
    fn binance_trade_server_to_client_mapping_preserves_nullable_fields() {
        let server_po = SpotStreamTradeRecordPo {
            id: 9,
            event_time: 1_700_000_000_001,
            inst_id: 123,
            trade_id: 42,
            price: 100.25,
            qty: 0.5,
            trade_time: None,
            is_buyer_maker: Some(true),
            created_at: 0,
        };

        let proto = BinanceTrade::from(server_po);
        let client_po = LocalBinanceTradePo::from_proto(proto, 9, 1_700_000_000_002);

        assert_eq!(client_po.trade_id, 42);
        assert_eq!(client_po.trade_time, None);
        assert_eq!(client_po.is_buyer_maker, Some(true));
    }

    // 测试目的：验证 Binance Funding Rate 服务端 PO 到 protobuf 的转换保留时间、费率和可空标记价格。
    // 准备数据/过程：构造包含 mark_price 的 FundingRatePo，并转换为 BinanceFundingRate 消息。
    // 预期结果：消息字段与服务端 PO 一致，FundingTime 保持毫秒精度，mark_price 为 Some。
    #[test]
    fn binance_funding_rate_server_po_maps_to_proto() {
        let server_po = crate::binance::models::po::FundingRatePo {
            id: 11,
            inst_id: 42,
            funding_rate: 0.0001,
            funding_time: 1_700_000_000_123,
            mark_price: Some(100.25),
        };

        let proto = BinanceFundingRate::from(server_po);

        assert_eq!(proto.id, 11);
        assert_eq!(proto.funding_time, 1_700_000_000_123);
        assert_eq!(proto.funding_rate, 0.0001);
        assert_eq!(proto.mark_price, Some(100.25));
    }

    // 测试目的：验证 OrderBook 完整快照从服务端模型经 protobuf 转为客户端 PO 时保留档位精度、顺序、instrument 关联和毫秒时间。
    // 准备数据/过程：构造包含多个买卖档位的 OrderBook，转为服务端 PO、HistoryEntry，再由客户端 PO 解码并生成 COPY 行。
    // 预期结果：更新时间、更新 ID、买卖档位价格/数量字符串及 instrument ID 均一致，JSONB 列数据可用且 CSV 列数匹配。
    #[test]
    fn binance_order_book_snapshot_maps_full_depth_without_precision_loss() {
        use crate::postgresql_db::CopyInsertable;
        use rust_decimal::prelude::FromStr;

        let order_book = OrderBook::new(
            "BTCUSDT",
            Depth {
                last_update_id: 1234,
                bids: vec![
                    (Decimal::from_str("100.00000001").unwrap(), Decimal::from_str("0.12345678").unwrap()),
                    (Decimal::from_str("99.5").unwrap(), Decimal::from_str("2.0").unwrap()),
                ],
                asks: vec![
                    (Decimal::from_str("100.5").unwrap(), Decimal::from_str("3.00000001").unwrap()),
                    (Decimal::from_str("101.0").unwrap(), Decimal::from_str("4").unwrap()),
                ],
            },
        )
        .unwrap();
        let snapshot_time = order_book.last_update_time;
        let server_po = BinanceOrderBookSnapshotPo::from_order_book(&order_book, 42);
        let entry = server_po.to_history_entry();
        let client_po = LocalBinanceOrderBookSnapshotPo::from_history_entry(&entry, 77);
        let csv_row = client_po.to_csv_row();
        let columns = <LocalBinanceOrderBookSnapshotPo as CopyInsertable>::columns();

        assert_eq!(entry.inst_id, 42);
        assert_eq!(HistoryType::try_from(entry.history_type).unwrap(), HistoryType::BinanceOrderBookSnapshot);
        assert_eq!(client_po.inst_id, 77);
        assert_eq!(client_po.server_id, server_po.id as u64);
        assert_eq!(client_po.snapshot_time, snapshot_time);
        assert_eq!(client_po.local_update_id, 1234);
        assert_eq!(client_po.bids[0].price, "100.00000001");
        assert_eq!(client_po.bids[0].quantity, "0.12345678");
        assert_eq!(client_po.bids[1].price, "99.5");
        assert_eq!(client_po.asks[0].price, "100.5");
        assert_eq!(client_po.asks[0].quantity, "3.00000001");
        let mut in_quotes = false;
        let mut fields = 1;
        let mut characters = csv_row.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '"' if in_quotes && characters.peek() == Some(&'"') => {
                    characters.next();
                }
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => fields += 1,
                _ => {}
            }
        }

        assert_eq!(fields, columns.split(',').count());
        assert!(csv_row.contains(r#"[{""price"":""100.00000001"",""quantity"":""0.12345678""}"#));
    }
}

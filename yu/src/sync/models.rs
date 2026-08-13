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
}

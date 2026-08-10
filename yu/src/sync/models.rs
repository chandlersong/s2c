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
}

use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::models::po::BinanceInstrument as BinanceInstrumentPo;
use crate::errors::YuError;
use crate::sync::models::grpc_sync::{Instrument, instrument::Payload};
use crate::sync::server::sync_server::{SyncInstrumentService, SyncInstrumentServiceTrait};
use async_trait::async_trait;
use log::error;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Status;

pub struct BinanceSyncInstrumentService {
    dashboard: Arc<BinanceDashboard>,
}

impl BinanceSyncInstrumentService {
    pub fn new(dashboard: Arc<BinanceDashboard>) -> SyncInstrumentService {
        Arc::new(Self { dashboard })
    }
}

#[async_trait]
impl SyncInstrumentServiceTrait for BinanceSyncInstrumentService {
    async fn list_instruments(&self) -> Result<HashMap<String, Instrument>, YuError> {
        let mut instruments = HashMap::new();

        for (market, symbols) in [("spot", self.dashboard.spot_all_symbols()), ("swap", self.dashboard.swap_all_symbols())] {
            let symbols = symbols.read().map_err(|e| {
                error!("list_binance_instruments lock error: {:?}", e);
                Status::internal(format!("list binance {market} instruments lock error: {e}"))
            })?;

            for symbol in symbols.iter() {
                let instrument_po = BinanceInstrumentPo::from(symbol);
                let key = format!("{market}:{}", instrument_po.symbol);
                let proto = crate::sync::models::grpc_sync::BinanceInstrument::from(instrument_po);
                let instrument = Instrument {
                    payload: Some(Payload::Binance(proto)),
                };

                if instruments.insert(key.clone(), instrument).is_some() {
                    error!("duplicate Binance instrument key: {}", key);
                    return Err(Status::internal(format!("duplicate binance instrument key: {key}")).into());
                }
            }
        }

        Ok(instruments)
    }

    fn error_log(&self) -> String {
        "error when list binance instruments".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binance::bn_consts::BN_SYMBOL_STATUS_TRADING;
    use crate::sync::models::grpc_sync::instrument::Payload;
    use yue::binance::bn_models::common::SymbolInfo;

    #[tokio::test]
    async fn spot_and_swap_symbols_with_same_name_are_listed_separately() {
        let spot_symbol = SymbolInfo {
            symbol: "BTCUSDT".to_string(),
            status: BN_SYMBOL_STATUS_TRADING.to_string(),
            base_asset: "BTC".to_string(),
            quote_asset: "USDT".to_string(),
            quote_asset_precision: 8,
            order_types: vec!["LIMIT".to_string()],
            symbol_type: "spot".to_string(),
            on_board_time: None,
        };
        let swap_symbol = SymbolInfo {
            symbol_type: "PERPETUAL".to_string(),
            on_board_time: Some(1_600_000_000_000),
            ..spot_symbol.clone()
        };
        let dashboard_raw = BinanceDashboard::new_with_data(vec![spot_symbol], vec![swap_symbol], 24).unwrap();
        let dashboard = Arc::new(dashboard_raw);
        let service = BinanceSyncInstrumentService::new(dashboard);

        let instruments = service.list_instruments().await.unwrap();

        assert_eq!(instruments.len(), 2);
        assert!(matches!(
            instruments.get("spot:BTCUSDT").and_then(|instrument| instrument.payload.as_ref()),
            Some(Payload::Binance(instrument)) if instrument.symbol == "BTCUSDT" && instrument.on_board_time.is_none()
        ));
        assert!(matches!(
            instruments.get("swap:BTCUSDT").and_then(|instrument| instrument.payload.as_ref()),
            Some(Payload::Binance(instrument)) if instrument.symbol == "BTCUSDT" && instrument.on_board_time == Some(1_600_000_000_000)
        ));
    }
}

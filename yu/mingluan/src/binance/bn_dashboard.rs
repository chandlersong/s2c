use crate::errors::MingLuanError;
use crate::exchange::ExchangeDashBoard;
use std::sync::{Arc, LockResult, RwLock};
use yue::binance::spots::{TradingSymbolInfo, get_trading_spot_symbols};

#[derive(Debug, Clone)]
struct ExchangeSpotVO {
    trading_symbols: Vec<TradingSymbolInfo>,
}

struct BinanceDashboard {
    spot_info: Arc<RwLock<ExchangeSpotVO>>,
}

impl ExchangeDashBoard for BinanceDashboard {
    type SpotDashBoard = ExchangeSpotVO;

    async fn spot_info(&self) -> Arc<RwLock<ExchangeSpotVO>> {
        self.spot_info.clone()
    }

    async fn refresh(&self) -> Result<(), MingLuanError> {
        let symbols = get_trading_spot_symbols(Some("ALL")).await?;

        match self.spot_info.write() {
            Ok(mut vo) => {
                vo.trading_symbols = symbols;
            }
            Err(_) => {
                return Err(MingLuanError::new("Failed to acquire write lock on spot_info"));
            }
        }
        Ok(())
    }
}

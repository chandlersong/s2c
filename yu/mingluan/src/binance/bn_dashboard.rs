use crate::actix_jobs::AsyncRepeatTask;
use crate::errors::MingLuanError;
use crate::exchange::ExchangeDashBoard;
use async_trait::async_trait;
use log::error;
use std::sync::{Arc, RwLock};
use yue::binance::history_data::{get_trading_spot_symbols, get_trading_swap_symbols, CONTRACT_TYPE_PERPETUAL};

#[derive(Debug, Clone)]
pub struct TradingSymbols {
    pub trading_spot_symbols: Vec<String>,
    pub trading_swap_symbols: Vec<String>,
}

#[derive(Clone)]
pub struct BinanceDashboard {
    spot_info: Arc<RwLock<TradingSymbols>>,
}

impl BinanceDashboard {
    pub fn new() -> Self {
        BinanceDashboard {
            spot_info: Arc::new(RwLock::new(TradingSymbols {
                trading_spot_symbols: vec![],
                trading_swap_symbols: vec![],
            })),
        }
    }
}

impl ExchangeDashBoard for BinanceDashboard {
    type TradingSymbol = TradingSymbols;

    fn spot_info(&self) -> Arc<RwLock<TradingSymbols>> {
        self.spot_info.clone()
    }
}

#[async_trait]
impl AsyncRepeatTask for BinanceDashboard {
    async fn execute(&self) -> Result<(), MingLuanError> {
        let (spot_res, swap_res) = tokio::join!(
            get_trading_spot_symbols(None),
            get_trading_swap_symbols(None, Some(CONTRACT_TYPE_PERPETUAL))
        );

        match (spot_res, swap_res) {
            (Ok(spot_symbols), Ok(swap_symbols)) => {
                if let Ok(mut vo) = self.spot_info.write() {
                    vo.trading_spot_symbols = spot_symbols.iter().map(|sym| sym.symbol.clone()).collect();
                    vo.trading_swap_symbols = swap_symbols.iter().map(|sym| sym.symbol.clone()).collect();
                    Ok(())
                } else {
                    error!("Failed to acquire write lock on spot_info");
                    Err(MingLuanError::CustomError("Failed to acquire write lock on spot_info".to_string()))
                }
            }
            (Err(e), _) => {
                error!("Error fetching trading spot symbols: {:?}", e);
                Err(e.into())
            }
            (_, Err(e)) => {
                error!("Error fetching trading swap symbols: {:?}", e);
                Err(e.into())
            }
        }
    }
}

use crate::actix_jobs::AsyncRepeatTask;
use crate::errors::MingLuanError;
use crate::exchange::ExchangeDashBoard;
use async_trait::async_trait;
use log::error;
use std::sync::{Arc, RwLock};
use yue::binance::spots::get_trading_spot_symbols;

#[derive(Debug, Clone)]
pub struct ExchangeSpotVO {
    pub trading_symbols: Vec<String>,
}

#[derive(Clone)]
pub struct BinanceDashboard {
    spot_info: Arc<RwLock<ExchangeSpotVO>>,
}

impl BinanceDashboard {
    pub fn new() -> Self {
        BinanceDashboard {
            spot_info: Arc::new(RwLock::new(ExchangeSpotVO { trading_symbols: vec![] })),
        }
    }
}

impl ExchangeDashBoard for BinanceDashboard {
    type SpotDashBoard = ExchangeSpotVO;

    fn spot_info(&self) -> Arc<RwLock<ExchangeSpotVO>> {
        self.spot_info.clone()
    }
}

#[async_trait]
impl AsyncRepeatTask for BinanceDashboard {
    async fn execute(&self) -> Result<(), MingLuanError> {
        match get_trading_spot_symbols(None).await {
            Ok(symbols) => {
                if let Ok(mut vo) = self.spot_info.write() {
                    vo.trading_symbols = symbols.iter().map(|sym| sym.symbol.clone()).collect();
                    Ok(())
                } else {
                    error!("Failed to acquire write lock on spot_info");
                    Err(MingLuanError::CustomError("Failed to acquire write lock on spot_info".to_string()))
                }
            }
            Err(e) => {
                error!("Error fetching trading symbols: {:?}", e);
                Err(e.into())
            }
        }
    }
}

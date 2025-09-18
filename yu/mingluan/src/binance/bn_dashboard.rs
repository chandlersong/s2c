use crate::actix_jobs::AsyncRepeatTask;
use crate::errors::MingLuanError;
use crate::exchange::ExchangeDashBoard;
use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use yue::binance::spots::get_trading_spot_symbols;

#[derive(Debug, Clone)]
pub struct ExchangeSpotVO {
    trading_symbols: Vec<String>,
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
            Ok(symbols) => match self.spot_info.write() {
                Ok(mut vo) => {
                    let mut trading_symbols = vec![];
                    for sym in symbols.iter() {
                        trading_symbols.push(sym.symbol.clone());
                    }
                    vo.trading_symbols = trading_symbols;
                    Ok(())
                }
                Err(_) => {
                    eprintln!("Failed to acquire write lock on spot_info");
                    Err(MingLuanError::CustomError("Failed to acquire write lock on spot_info".to_string()))
                }
            },
            Err(e) => {
                eprintln!("Error fetching trading symbols: {:?}", e);
                Err(e.into())
            }
        }
    }
}

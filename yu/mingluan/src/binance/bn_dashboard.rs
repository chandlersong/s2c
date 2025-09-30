use crate::actix_jobs::AsyncRepeatTask;
use crate::errors::MingLuanError;
use crate::exchange::ExchangeDashBoard;
use async_trait::async_trait;
use log::error;
use std::sync::{Arc, RwLock};
use yue::binance::history_data::{get_trading_spot_symbols, get_trading_swap_symbols, CONTRACT_TYPE_PERPETUAL};

/// TODO： 这里返回对象
/// 这里用户可以获取字符串的symbol,也能获取一些计算其他需要的信息，比如说
/// 1. swap的上市时间
/// 2. 最小交易单位
#[derive(Debug, Clone)]
pub struct TradingSymbol {
    pub symbol: String,
    pub on_board_time: Option<u64>,
    pub quote_asset: String, //报价资产
}

#[derive(Clone)]
pub struct BinanceDashboard {
    spot_symbols: Arc<RwLock<Vec<TradingSymbol>>>,
    swap_symbols: Arc<RwLock<Vec<TradingSymbol>>>,
}

impl BinanceDashboard {
    pub fn new() -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
        }
    }

    #[cfg(test)]
    pub fn new_with_data(spot_symbol: Vec<TradingSymbol>, swap_symbol: Vec<TradingSymbol>) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(spot_symbol)),
            swap_symbols: Arc::new(RwLock::new(swap_symbol)),
        }
    }
}

impl ExchangeDashBoard for BinanceDashboard {
    type TradingSymbol = TradingSymbol;

    fn spot_symbols(&self) -> Arc<RwLock<Vec<TradingSymbol>>> {
        self.spot_symbols.clone()
    }

    fn swap_symbols(&self) -> Arc<RwLock<Vec<TradingSymbol>>> {
        self.swap_symbols.clone()
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
                let trading_spot_symbols: Vec<TradingSymbol> = spot_symbols
                    .iter()
                    .map(|sym| TradingSymbol {
                        symbol: sym.symbol.clone(),
                        on_board_time: None,
                        quote_asset: sym.quote_asset.clone(),
                    })
                    .collect();
                let trading_swap_symbols: Vec<TradingSymbol> = swap_symbols
                    .iter()
                    .map(|sym| TradingSymbol {
                        symbol: sym.symbol.clone(),
                        on_board_time: sym.on_board_time,
                        quote_asset: sym.quote_asset.clone(),
                    })
                    .collect();

                *self.spot_symbols.write().unwrap() = trading_spot_symbols;
                *self.swap_symbols.write().unwrap() = trading_swap_symbols;
                Ok(())
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

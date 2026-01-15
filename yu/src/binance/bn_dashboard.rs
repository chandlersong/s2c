use crate::actix_jobs::AsyncRepeatTask;
use crate::errors::YuError;
use crate::exchange::ExchangeDashBoard;
use actix::{Actor, Context, Handler, Message};
use async_trait::async_trait;
use log::error;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use yue::binance::history_data::{get_trading_spot_symbols, get_trading_swap_symbols, CONTRACT_TYPE_PERPETUAL};
use yue::binance::order_book::{OrderBook, OrderBookSnapshotMsg};

#[derive(Debug, Clone)]
pub struct TradingSymbol {
    pub symbol: String,
    pub on_board_time: Option<u64>,
    pub quote_asset: String, //报价资产
}

//NEXT：把这些存入数据库
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
    async fn execute(&self) -> Result<(), YuError> {
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

    fn task_name(&self) -> &str {
        "binance dashboard"
    }
}

#[derive(Message)]
#[rtype(result = "Option<Arc<OrderBook>>")]
pub struct QueryDepth {
    pub symbol: String,
}

#[derive(Message)]
#[rtype(result = "Vec<String>")]
pub struct QueryAllSymbols;

#[derive(Message)]
#[rtype(result = "Vec<Arc<OrderBook>>")]
pub struct QueryBatchDepths {
    pub symbols: Vec<String>,
}

pub struct MarketDepthDashBoard {
    depths: HashMap<String, Arc<OrderBook>>,
}

impl MarketDepthDashBoard {
    pub fn new() -> Self {
        MarketDepthDashBoard { depths: HashMap::new() }
    }
}

impl Actor for MarketDepthDashBoard {
    type Context = Context<Self>;
}

impl Handler<OrderBookSnapshotMsg> for MarketDepthDashBoard {
    type Result = ();

    fn handle(&mut self, msg: OrderBookSnapshotMsg, _ctx: &mut Context<Self>) -> Self::Result {
        let order_book = msg.0;
        self.depths.insert(order_book.symbol.clone(), order_book);
    }
}

impl Handler<QueryDepth> for MarketDepthDashBoard {
    type Result = Option<Arc<OrderBook>>;

    fn handle(&mut self, msg: QueryDepth, _ctx: &mut Context<Self>) -> Self::Result {
        self.depths.get(&msg.symbol).cloned()
    }
}

impl Handler<QueryAllSymbols> for MarketDepthDashBoard {
    type Result = Vec<String>;

    fn handle(&mut self, _msg: QueryAllSymbols, _ctx: &mut Context<Self>) -> Self::Result {
        self.depths.keys().cloned().collect()
    }
}

impl Handler<QueryBatchDepths> for MarketDepthDashBoard {
    type Result = Vec<Arc<OrderBook>>;

    fn handle(&mut self, msg: QueryBatchDepths, _ctx: &mut Context<Self>) -> Self::Result {
        msg.symbols.iter().filter_map(|symbol| self.depths.get(symbol).cloned()).collect()
    }
}

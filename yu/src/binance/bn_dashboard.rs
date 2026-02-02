use crate::errors::YuError;
use crate::exchange::ExchangeDashBoard;
use actix::{Actor, Addr, Context, Handler, Message};
use async_trait::async_trait;
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use log::{error, info};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use yue::binance::history_data::{get_trading_spot_symbols, get_trading_swap_symbols, CONTRACT_TYPE_PERPETUAL};
use yue::binance::order_book::{OrderBook, OrderBookSnapshotMsg};
use yue::binance::websocket_handler::TradingSymbolRefresher;

#[derive(Debug, Clone)]
pub struct TradingSymbol {
    pub symbol: String,
    pub on_board_time: Option<u64>,
    pub quote_asset: String, //报价资产
    pub status: String,
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

///
/// 暂时所有的交易对都以USDT报价资产为准
///
impl TradingSymbolRefresher for BinanceDashboard {
    fn list_spot(&self) -> Vec<String> {
        self.spot_symbols
            .read()
            .unwrap()
            .clone()
            .iter()
            .filter(|symbol| symbol.status == "TRADING")
            .filter(|symbol| symbol.quote_asset == "USDT")
            .map(|s| s.symbol.clone())
            .collect()
    }

    fn list_swap(&self) -> Vec<String> {
        self.swap_symbols
            .read()
            .unwrap()
            .clone()
            .iter()
            .filter(|symbol| symbol.status == "TRADING")
            .filter(|symbol| symbol.quote_asset == "USDT")
            .map(|s| s.symbol.clone())
            .collect()
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
    ///
    /// TODO：
    /// 1，swap根据数据，判断上架和下架操作
    ///
    async fn execute(&self) -> Result<(), LiError> {
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
                        status: sym.status.clone(),
                    })
                    .collect();
                let trading_swap_symbols: Vec<TradingSymbol> = swap_symbols
                    .iter()
                    .map(|sym| TradingSymbol {
                        symbol: sym.symbol.clone(),
                        on_board_time: sym.on_board_time,
                        quote_asset: sym.quote_asset.clone(),
                        status: sym.status.clone(),
                    })
                    .collect();

                *self.spot_symbols.write().unwrap() = trading_spot_symbols;
                *self.swap_symbols.write().unwrap() = trading_swap_symbols;
                Ok(())
            }
            (Err(e), _) => {
                error!("Error fetching trading spot symbols: {:?}", e);
                Err(LiError::CustomError(format!("获取现货交易对失败: {}", e)))
            }
            (_, Err(e)) => {
                error!("Error fetching trading swap symbols: {:?}", e);
                Err(LiError::CustomError(format!("获取合约交易对失败: {}", e)))
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

/// 全局MarketDepthDashBoard单例存储
static MARKET_DEPTH_DASHBOARD: OnceLock<Addr<MarketDepthDashBoard>> = OnceLock::new();

/// 初始化全局MarketDepthDashBoard单例
/// 应该在应用启动时调用一次
pub fn init_market_depth_dashboard(addr: Addr<MarketDepthDashBoard>) -> Result<(), Addr<MarketDepthDashBoard>> {
    MARKET_DEPTH_DASHBOARD.set(addr)
}

/// 获取全局MarketDepthDashBoard单例
/// 如果未初始化，返回错误
pub fn get_market_depth_dashboard() -> Result<Addr<MarketDepthDashBoard>, YuError> {
    MARKET_DEPTH_DASHBOARD
        .get()
        .cloned()
        .ok_or_else(|| YuError::CustomError("MarketDepthDashBoard未初始化，请先调用init_market_depth_dashboard".to_string()))
}

/// 市场深度仪表盘，负责存储和查询订单簿快照。
///
/// 这个Actor接收来自OrderBookService的订单簿快照，并提供同步查询接口。
///
/// 性能设计说明：
/// - 与OrderBookService保持分离的Actor线程，避免互相阻塞
/// - OrderBookService处理高频深度更新（websocket实时推送）
/// - MarketDepthDashBoard处理查询请求（纯读操作）
/// - 通过Arc<OrderBook>共享数据，无复制成本
///
/// 不推荐合并的原因：
/// 1. 深度更新是高频消息（每秒数千条），查询是阻塞操作
/// 2. 如果合并，查询请求会阻塞深度更新处理，导致订单簿更新延迟
/// 3. 在高交易量场景下，这个延迟会积累，最坏情况下从毫秒级增加到秒级
/// 4. 分离设计允许独立优化：OrderBookService专注写操作，MarketDepthDashBoard专注读操作
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
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("OrderBookService 启动");

        // 启动初始化Actor

        ctx.set_mailbox_capacity(1000);
    }
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

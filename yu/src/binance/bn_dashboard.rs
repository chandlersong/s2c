use crate::errors::YuError;
use crate::exchange::ExchangeDashBoard;
use actix::{Actor, Addr, Context, Handler, Message};
use async_trait::async_trait;
use li::actix_jobs::AsyncRepeatTask;
use li::errors::LiError;
use li::tools::time::{unix_time_now_u64_utc, UnixTimeStamp};
use log::{error, info};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use yue::binance::bn_models::spot_restful::ExchangeInfo;
use yue::binance::bn_models::swap_restful::SwapExchangeInfo;
use yue::binance::bn_restful_commands::{
    execute_json_request, BINANCE_SPOT_BASE, BINANCE_SWAP_BASE, SPOT_EXCHANGE_COMMAND, SPOT_RATE_PER_MINUTE, SWAP_EXCHANGE_COMMAND,
};
use yue::binance::history_data::{get_trading_spot_symbols, get_trading_swap_symbols, TradingSymbolInfo, CONTRACT_TYPE_PERPETUAL};
use yue::binance::order_book::{OrderBook, OrderBookSnapshotMsg};
use yue::binance::websocket_actor::TradingSymbolRefresher;
use yue::errors::YueError;
use yue::http_client::get_http_client;
use yue::models::HistoryInterval;

#[derive(Debug, Clone)]
pub struct TradingSymbol {
    pub symbol: String,
    pub on_board_time: Option<u64>,
    pub quote_asset: String, //报价资产
    pub status: String,
}

type BinanceSnapshot = watch::Sender<Arc<BinanceDashboardSnapShot>>;
#[derive(Clone)]
pub struct BinanceDashboardSnapShot {
    pub spot_trading_symbols: Vec<TradingSymbol>,
    pub swap_trading_symbols: Vec<TradingSymbol>,
    pub timestamp: UnixTimeStamp,
}

impl BinanceDashboardSnapShot {
    pub fn new(spot_trading_symbols: Vec<TradingSymbol>, swap_trading_symbols: Vec<TradingSymbol>) -> Self {
        Self {
            spot_trading_symbols,
            swap_trading_symbols,
            timestamp: unix_time_now_u64_utc(),
        }
    }
}

pub type BinanceDashboardWatcher = watch::Sender<Arc<BinanceDashboardSnapShot>>;

//FUTURE：把这些存入数据库
#[derive(Clone)]
pub struct BinanceDashboard {
    spot_symbols: Arc<RwLock<Vec<TradingSymbol>>>,
    swap_symbols: Arc<RwLock<Vec<TradingSymbol>>>,
    data_retention_hours: u64,
    debug_mood: bool,
}

impl BinanceDashboard {
    pub fn new(data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: false,
        }
    }

    ///
    /// 主要本地的初始化的request的访问很长。所以写了一个debug模式。
    /// 所有的改动，手工调用
    /// Mock目录：
    /// 1. 各个的exchange info的update。从本地直接读取文件。
    ///
    #[deprecated]
    pub fn debug_mode(data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(vec![])),
            swap_symbols: Arc::new(RwLock::new(vec![])),
            data_retention_hours,
            debug_mood: true,
        }
    }

    pub fn new_with_data(spot_symbol: Vec<TradingSymbol>, swap_symbol: Vec<TradingSymbol>, data_retention_hours: u64) -> Self {
        BinanceDashboard {
            spot_symbols: Arc::new(RwLock::new(spot_symbol)),
            swap_symbols: Arc::new(RwLock::new(swap_symbol)),
            data_retention_hours,
            debug_mood: false,
        }
    }

    async fn query_spot_exchange_info() -> Result<ExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(SPOT_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<ExchangeInfo>(&SPOT_EXCHANGE_COMMAND, rb, None).await
    }

    async fn query_swap_exchange_info() -> Result<SwapExchangeInfo, YueError> {
        let client = get_http_client();
        let rb = client.get(SWAP_EXCHANGE_COMMAND.as_ref().as_str());
        execute_json_request::<SwapExchangeInfo>(&SWAP_EXCHANGE_COMMAND, rb, None).await
    }

    async fn refresh_rate_limit(spot_exchange: &ExchangeInfo, swap_exchange: &SwapExchangeInfo) {
        let spot_request_limit = spot_exchange
            .rate_limits
            .iter()
            .filter(|r| r.rate_limit_type == "REQUEST_WEIGHT" && r.interval == "MINUTE")
            .map(|r| r.limit)
            .next()
            .unwrap_or(SPOT_RATE_PER_MINUTE as i32);
        let swap_request_limit = swap_exchange
            .rate_limits
            .iter()
            .filter(|r| r.rate_limit_type == "REQUEST_WEIGHT" && r.interval == "MINUTE")
            .map(|r| r.limit)
            .next()
            .unwrap_or(SPOT_RATE_PER_MINUTE as i32);
        info!("refresh spot rate limit {},swap rate limit {}", spot_request_limit, swap_request_limit);
        BINANCE_SPOT_BASE.clone().refresh_rate_limit(spot_request_limit as u32, None).await;
        BINANCE_SWAP_BASE.clone().refresh_rate_limit(swap_request_limit as u32, None).await;
    }

    fn refresh_trading_symbol(
        &self,
        spot_res: Result<Vec<TradingSymbolInfo>, YueError>,
        swap_res: Result<Vec<TradingSymbolInfo>, YueError>,
    ) -> Result<(Vec<TradingSymbol>, Vec<TradingSymbol>), LiError> {
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

                *self.spot_symbols.write().unwrap() = trading_spot_symbols.clone();
                *self.swap_symbols.write().unwrap() = trading_swap_symbols.clone();
                Ok((trading_spot_symbols, trading_swap_symbols))
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

    pub fn read_spot_exchange_info_from_file_sync<P: AsRef<Path>, E: DeserializeOwned>(path: P) -> Result<E, YueError> {
        let s = fs::read_to_string(path)?; // std::io::Error -> YueError::IoError
        let info: E = serde_json::from_str(&s)?; // serde_json::Error -> YueError::SerdeError
        Ok(info)
    }
    pub async fn execute(&self) -> Result<Arc<BinanceDashboardSnapShot>, LiError> {
        let (spot_exchange, swap_exchange) = if self.debug_mood {
            let spot = Self::read_spot_exchange_info_from_file_sync("testdata/exchange_data/spot_exchange.json");
            let swap = Self::read_spot_exchange_info_from_file_sync("testdata/exchange_data/swap_exchange.json");
            (spot, swap)
        } else {
            let (spot_exchange, swap_exchange) = tokio::join!(Self::query_spot_exchange_info(), Self::query_swap_exchange_info());
            (spot_exchange, swap_exchange)
        };

        match (spot_exchange, swap_exchange) {
            (Ok(spot), Ok(swap)) => {
                // refresh limit
                //TODO：能正常下载后，再把这个功能加上。
                Self::refresh_rate_limit(&spot, &swap).await;
                // refresh symbol
                let spot_res = get_trading_spot_symbols(spot, None).await;
                let swap_res = get_trading_swap_symbols(swap, None, Some(CONTRACT_TYPE_PERPETUAL)).await;

                let res = match self.refresh_trading_symbol(spot_res, swap_res) {
                    Ok((spot_symbols, swap_symbols)) => BinanceDashboardSnapShot::new(spot_symbols, swap_symbols),
                    Err(e) => return Err(LiError::CustomError(format!("刷新交易符号失败: {}", e))),
                };

                Ok(Arc::new(res))
            }
            (Err(e), _) => Err(LiError::CustomError(format!("获取现货交易所信息失败: {}", e))),
            (_, Err(e)) => Err(LiError::CustomError(format!("获取永续交易所信息失败: {}", e))),
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

    fn spot_all_symbols(&self) -> Arc<RwLock<Vec<TradingSymbol>>> {
        self.spot_symbols.clone()
    }

    fn swap_all_symbols(&self) -> Arc<RwLock<Vec<TradingSymbol>>> {
        self.swap_symbols.clone()
    }

    fn spot_trading_symbols(&self) -> Vec<TradingSymbol> {
        self.spot_symbols
            .read()
            .unwrap()
            .clone()
            .iter()
            .filter(|symbol| symbol.status == "TRADING")
            .filter(|symbol| symbol.quote_asset == "USDT")
            .map(|s| s.clone())
            .collect()
    }

    fn swap_trading_symbols(&self) -> Vec<TradingSymbol> {
        self.swap_symbols
            .read()
            .unwrap()
            .clone()
            .iter()
            .filter(|symbol| symbol.status == "TRADING")
            .filter(|symbol| symbol.quote_asset == "USDT")
            .map(|s| s.clone())
            .collect()
    }

    ///
    /// 1. 获取当前时间。然后减去data_retention_hours，得到应该保留的最早时间戳
    /// 2. 根据interval调整时间戳进行调整
    ///
    /// interval: 默认值是五分钟
    /// 返回标准应该保留的最大时间
    ///
    fn get_earliest_timestamp(&self, interval: Option<HistoryInterval>) -> Option<u64> {
        // 获取当前 Unix 毫秒时间
        let now_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_millis() as u64,
            Err(_) => return None,
        };

        // 计算保留时长对应的毫秒数（防溢出）
        let retention_ms = self.data_retention_hours.saturating_mul(3600).saturating_mul(1000);

        // 计算最早保留的时间戳（不小于0）
        let earliest = now_ms.saturating_sub(retention_ms);

        // 使用传入的 interval 对齐时间戳，默认使用 FiveMinutes
        let interval_to_use = interval.unwrap_or(HistoryInterval::FiveMinutes);
        let aligned = interval_to_use.get_close_unix_ms(earliest);

        Some(aligned)
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

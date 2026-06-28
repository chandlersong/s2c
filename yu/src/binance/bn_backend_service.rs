use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::models::po::{FundingRatePo, KlinePo};
use crate::binance::models::SpotStreamTradeRecordPo;
use crate::binance::trading_service::TradingService;
use crate::config::get_config;
use crate::duck_db_tables::{DuckDBOneTable, DuckTableTableChannel};
use std::sync::OnceLock;
use tokio::sync::OnceCell;
use yue::binance::order_book::OrderBookService;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_FUNDING_RATE_TABLE: OnceLock<DuckTableTableChannel<FundingRatePo>> = OnceLock::new();
pub(crate) static SPOT_TRADE_TABLE: OnceLock<DuckTableTableChannel<SpotStreamTradeRecordPo>> = OnceLock::new();
pub(crate) static SPOT_ORDER_BOOK: OnceCell<OrderBookService> = OnceCell::const_new();
pub(crate) static SPOT_TRADING_SERVICE: OnceCell<TradingService> = OnceCell::const_new();

pub async fn get_spot_order_book_service() -> &'static OrderBookService {
    SPOT_ORDER_BOOK
        .get_or_init(init_spot_order_book_service) // ← 这里会自动异步初始化，只执行一次
        .await
}

async fn init_spot_order_book_service() -> OrderBookService {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    OrderBookService::spot(proxy).await
}

pub async fn get_spot_trading_service() -> &'static TradingService {
    SPOT_TRADING_SERVICE
        .get_or_init(init_spot_trading_service) // ← 这里会自动异步初始化，只执行一次
        .await
}

async fn init_spot_trading_service() -> TradingService {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    TradingService::spot(proxy).await
}

pub fn get_spot_kline_table() -> DuckTableTableChannel<KlinePo> {
    SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<KlinePo>::start_new(BinanceTables::SpotKline))
        .clone()
}

pub fn get_spot_trading_table() -> DuckTableTableChannel<SpotStreamTradeRecordPo> {
    SPOT_TRADE_TABLE
        .get_or_init(|| DuckDBOneTable::<SpotStreamTradeRecordPo>::start_new(BinanceTables::SpotTrade))
        .clone()
}

pub fn get_swap_kline_table() -> DuckTableTableChannel<KlinePo> {
    SWAP_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<KlinePo>::start_new(BinanceTables::SwapKline))
        .clone()
}

pub fn get_swap_funding_rate_table() -> DuckTableTableChannel<FundingRatePo> {
    SWAP_FUNDING_RATE_TABLE
        .get_or_init(|| DuckDBOneTable::<FundingRatePo>::start_new(BinanceTables::SwapFundingRate))
        .clone()
}

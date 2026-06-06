use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::bn_duck_db::{DuckDBOneTable, DuckTableTableChannel};
use crate::binance::models::po::{FundingRatePo, KlinePo};
use crate::config::get_config;
use once_cell::unsync::Lazy;
use std::sync::{Arc, OnceLock};
use tokio::sync::OnceCell;
use yue::binance::order_book::OrderBookService;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_FUNDING_RATE_TABLE: OnceLock<DuckTableTableChannel<FundingRatePo>> = OnceLock::new();
pub(crate) static SPOT_ORDER_BOOK: OnceCell<OrderBookService> = OnceCell::const_new();

pub async fn get_spot_order_book() -> &'static OrderBookService {
    SPOT_ORDER_BOOK
        .get_or_init(init_spot_order_book) // ← 这里会自动异步初始化，只执行一次
        .await
}

async fn init_spot_order_book() -> OrderBookService {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    OrderBookService::spot(proxy).await
}

pub fn get_spot_kline_table() -> DuckTableTableChannel<KlinePo> {
    SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<KlinePo>::start_new(BinanceTables::SpotKline))
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

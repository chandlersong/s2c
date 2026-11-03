use crate::binance::db_consts::BinanceTables;

use crate::binance::models::po::{FundingRatePo, KlinePo};
use crate::config::get_config;
use crate::duck_db_tables::{DuckDBOneTable, DuckTableTableChannel};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::OnceCell;
use yue::binance::order_book::OrderBookService;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_FUNDING_RATE_TABLE: OnceLock<DuckTableTableChannel<FundingRatePo>> = OnceLock::new();
pub(crate) static SPOT_ORDER_BOOK: OnceCell<Arc<OrderBookService>> = OnceCell::const_new();

pub async fn get_spot_order_book_service() -> Arc<OrderBookService> {
    SPOT_ORDER_BOOK
        .get_or_init(init_spot_order_book_service) // ← 这里会自动异步初始化，只执行一次
        .await
        .clone()
}

async fn init_spot_order_book_service() -> Arc<OrderBookService> {
    let app_config = get_config();
    let proxy = app_config.proxy_url.clone();
    Arc::new(OrderBookService::spot(proxy).await)
}

pub fn get_spot_kline_table() -> DuckTableTableChannel<KlinePo> {
    SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| {
            DuckDBOneTable::<KlinePo, BinanceTables>::start_new_with_flush_config(BinanceTables::SpotKline, None, Duration::from_secs(1), 600)
        })
        .clone()
}

pub fn get_swap_kline_table() -> DuckTableTableChannel<KlinePo> {
    SWAP_BINANCE_KLINE_TABLE
        .get_or_init(|| {
            DuckDBOneTable::<KlinePo, BinanceTables>::start_new_with_flush_config(BinanceTables::SwapKline, None, Duration::from_secs(1), 600)
        })
        .clone()
}

pub fn get_swap_funding_rate_table() -> DuckTableTableChannel<FundingRatePo> {
    SWAP_FUNDING_RATE_TABLE
        .get_or_init(|| {
            DuckDBOneTable::<FundingRatePo, BinanceTables>::start_new_with_flush_config(
                BinanceTables::SwapFundingRate,
                None,
                Duration::from_secs(5),
                10000,
            )
        })
        .clone()
}

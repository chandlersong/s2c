use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::bn_duck_db::{BinanceKlineDataExecutor, DuckDBOneTable, DuckTableTableChannel};
use crate::binance::models::po::{FundingRatePo, KlinePo};
use std::sync::OnceLock;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::spot_websocket_stream::SpotKlineData;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::query_message::DataSourceExecutorTrait;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<BinanceKlineDataExecutor> = OnceLock::new();

pub(crate) static RAW_SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub fn get_raw_spot_kline_table() -> DuckTableTableChannel<KlinePo> {
    RAW_SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<KlinePo>::start_new(BinanceTables::SpotKline))
        .clone()
}

pub fn get_spot_kline_table_addr() -> BinanceKlineDataExecutor {
    SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| BinanceKlineDataExecutor::new(get_raw_spot_kline_table()))
        .clone()
}

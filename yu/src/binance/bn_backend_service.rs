use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::bn_duck_db::DuckDBOneTable;
use crate::binance::models::po::{FundingRatePo, KlinePo};
use std::sync::OnceLock;
use yue::binance::bn_models::spot_restful::BinanceKline;
use yue::binance::bn_models::swap_restful::FundingRate;
use yue::query_message::DataSourceExecutor;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<DataSourceExecutor<BinanceKline>> = OnceLock::new();
pub(crate) static SWAP_BINANCE_KLINE_TABLE: OnceLock<DataSourceExecutor<BinanceKline>> = OnceLock::new();
pub(crate) static SWAP_BINANCE_FUNDING_RATE_TABLE: OnceLock<DataSourceExecutor<FundingRate>> = OnceLock::new();

pub fn get_spot_kline_table() -> DataSourceExecutor<BinanceKline> {
    SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<BinanceKline, KlinePo>::start_new(BinanceTables::SpotKline))
        .clone()
}

pub fn get_swap_kline_table_addr() -> DataSourceExecutor<BinanceKline> {
    SWAP_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<BinanceKline, KlinePo>::start_new(BinanceTables::SwapKline))
        .clone()
}

pub fn get_swap_funding_rate_table_addr() -> DataSourceExecutor<FundingRate> {
    SWAP_BINANCE_FUNDING_RATE_TABLE
        .get_or_init(|| DuckDBOneTable::<FundingRate, FundingRatePo>::start_new(BinanceTables::SwapFundingRate))
        .clone()
}

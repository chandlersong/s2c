use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::bn_duck_db::{DuckDBOneTable, DuckTableTableChannel};
use crate::binance::models::po::{FundingRatePo, KlinePo};
use std::sync::OnceLock;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub(crate) static SWAP_FUNDING_RATE_TABLE: OnceLock<DuckTableTableChannel<FundingRatePo>> = OnceLock::new();

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

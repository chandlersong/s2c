use crate::binance::binance_db_consts::BinanceTables;

use crate::binance::bn_duck_db::{DuckDBOneTable, DuckTableTableChannel};
use crate::binance::models::po::KlinePo;
use std::sync::OnceLock;

///
/// 后台运行，一些以actor为主的全局唯一的进程
///

pub(crate) static RAW_SPOT_BINANCE_KLINE_TABLE: OnceLock<DuckTableTableChannel<KlinePo>> = OnceLock::new();

pub fn get_raw_spot_kline_table() -> DuckTableTableChannel<KlinePo> {
    RAW_SPOT_BINANCE_KLINE_TABLE
        .get_or_init(|| DuckDBOneTable::<KlinePo>::start_new(BinanceTables::SpotKline))
        .clone()
}

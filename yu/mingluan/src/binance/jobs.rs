use crate::actix_jobs::{AsyncRepeatTask, CronActor};
use crate::binance::binance_consts::BinanceTables::{SpotKline, SwapFundingRate, SwapKline};
use crate::binance::binance_consts::ALL_BINANCE_TABLES;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::history_task::{DuckDBHistoryDataWriter, FundingRatePo, KlinePo, UpdateHistoryTask};
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::{CloneHistoryFetcherFactory, ExchangeDashBoard};
use actix::Actor;
use duckdb::Connection;
use std::sync::Arc;
use yue::binance::bn_models::{BinanceKline, FundingRate, SymbolType};
use yue::binance::bn_restful_commands::{SPOT_KLINE_COMMAND, SWAP_FUNDING_RATE_COMMAND, SWAP_KLINE_COMMAND};
use yue::binance::history_data::{KlineParams, SimpleHistoryFetcher};
///
/// NOTE: 加入的功能
/// 1. 检测数据完整性的进程。
///
///
pub async fn start_bn_jobs() -> Result<(), MingLuanError> {
    let dashboard = BinanceDashboard::new();
    //每六个小时更新一次。因为这样频率不要那么高
    dashboard.execute().await?;

    let trading_symbols = dashboard.trading_symbols();
    initial_table()?;
    let base_spot_kline_fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_COMMAND);
    let spot_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_spot_kline_fetcher);

    let spot_data_writer = Arc::new(DuckDBHistoryDataWriter::new(
        DBProvider::default(),
        SpotKline.table_name(),
        SymbolType::Spot,
    ));

    let spot_kline_task = UpdateHistoryTask::<_, _, KlinePo, BinanceKline>::new(spot_kline_fetcher, trading_symbols.clone(), spot_data_writer);
    spot_kline_task.execute().await?;

    //TODO： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", dashboard, "update exchange info").start();
    let _ = CronActor::new("10 0 * * * * *", spot_kline_task, "fetch spot kline").start();

    let base_swap_kline_fetcher = SimpleHistoryFetcher::new(&SWAP_KLINE_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);
    let swap_kline_writer = Arc::new(DuckDBHistoryDataWriter::new(
        DBProvider::default(),
        SwapKline.table_name(),
        SymbolType::Swap,
    ));
    let swap_kline_task = UpdateHistoryTask::<_, _, KlinePo, BinanceKline>::new(swap_kline_fetcher, trading_symbols.clone(), swap_kline_writer);
    let _ = CronActor::new("10 0 * * * * *", swap_kline_task, "fetch swap kline").start();

    //TODO: 写一个资金费率的专用的param
    let base_swap_funding_rate_fetcher = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    let swap_funding_rate_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, FundingRate> =
        CloneHistoryFetcherFactory::new(base_swap_funding_rate_fetcher);

    let swap_funding_rate_writer = Arc::new(DuckDBHistoryDataWriter::new(
        DBProvider::default(),
        SwapFundingRate.table_name(),
        SymbolType::Swap,
    ));
    let swap_kline_task =
        UpdateHistoryTask::<_, _, FundingRatePo, FundingRate>::new(swap_funding_rate_fetcher, trading_symbols.clone(), swap_funding_rate_writer);
    let _ = CronActor::new("10 0 * * * * *", swap_kline_task, "fetch swap funding rate").start();

    Ok(())
}

fn table_exists(conn: &Connection, table_name: &str) -> Result<bool, MingLuanError> {
    let check_sql = format!("SELECT name FROM sqlite_master WHERE type='table' AND name='{}'", table_name);
    let mut stmt = conn.prepare(&check_sql)?;
    let mut rows = stmt.query([])?;
    Ok(rows.next()?.is_some())
}

fn initial_table() -> Result<(), MingLuanError> {
    let conn = DBProvider::default().acquire()?;
    for table in ALL_BINANCE_TABLES.iter() {
        let table_name = table.table_name();
        if !table_exists(&conn, &table_name)? {
            // 表不存在，执行建表
            conn.execute(table.create_table_statement().as_str(), [])?;
        }
    }
    Ok(())
}

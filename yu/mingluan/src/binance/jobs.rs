use crate::actix_jobs::{AsyncRepeatTask, CronActor};
use crate::binance::binance_consts::BinanceTables::{SpotKline, SwapFundingRate, SwapKline};
use crate::binance::binance_consts::ALL_BINANCE_TABLES;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::history_task::{DuckDBHistoryDataWriter, FundingRatePo, InitialHistoryTask, KlinePo};
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::CloneHistoryFetcherFactory;
use actix::Actor;
use duckdb::Connection;
use std::sync::Arc;
use yue::binance::bn_models::{BinanceKline, FundingRate, SymbolType};
use yue::binance::bn_restful_commands::{SPOT_KLINE_HISTORY_COMMAND, SWAP_FUNDING_RATE_COMMAND, SWAP_KLINE_HISTORY_COMMAND};
use yue::binance::history_data::{KlineParams, SimpleHistoryFetcher};
///
/// NEXT: 加入的功能
/// 1. 检测数据完整性的进程。
/// 2. 初始化并行执行。
///     - spot和swap的kline阻塞
///     - funding rate非阻塞
///
///
pub async fn start_bn_jobs() -> Result<(), MingLuanError> {
    let dashboard = BinanceDashboard::new();
    //每六个小时更新一次。因为这样频率不要那么高
    dashboard.execute().await?;
    let update_dashboard_task = dashboard.clone();
    let dash_board = Arc::new(dashboard);
    initial_table()?;
    let base_spot_kline_fetcher = SimpleHistoryFetcher::new(&SPOT_KLINE_HISTORY_COMMAND);
    let spot_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_spot_kline_fetcher);

    let spot_data_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SpotKline, SymbolType::Spot));

    let spot_kline_task = InitialHistoryTask::<_, _, KlinePo, BinanceKline, BinanceDashboard>::new(
        spot_kline_fetcher,
        dash_board.clone(),
        spot_data_writer,
        "refresh spot kline data".to_string(),
    );
    spot_kline_task.execute().await?;

    let base_swap_kline_fetcher = SimpleHistoryFetcher::new(&SWAP_KLINE_HISTORY_COMMAND);
    let swap_kline_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, BinanceKline> =
        CloneHistoryFetcherFactory::new(base_swap_kline_fetcher);
    let swap_kline_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapKline, SymbolType::Swap));
    let swap_kline_task = InitialHistoryTask::<_, _, KlinePo, BinanceKline, BinanceDashboard>::new(
        swap_kline_fetcher,
        dash_board.clone(),
        swap_kline_writer,
        "refresh swap kline data".to_string(),
    );
    swap_kline_task.execute().await?;

    //NEXT: 写一个资金费率的专用的param
    let base_swap_funding_rate_fetcher = SimpleHistoryFetcher::new(&SWAP_FUNDING_RATE_COMMAND);
    let swap_funding_rate_fetcher: CloneHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, FundingRate> =
        CloneHistoryFetcherFactory::new(base_swap_funding_rate_fetcher);
    let swap_funding_rate_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SwapFundingRate, SymbolType::Swap));
    let swap_funding_rate_task = InitialHistoryTask::<_, _, FundingRatePo, FundingRate, BinanceDashboard>::new(
        swap_funding_rate_fetcher,
        dash_board.clone(),
        swap_funding_rate_writer,
        "refresh swap funding rate".to_string(),
    );
    swap_funding_rate_task.execute().await?;
    //PLAN： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", update_dashboard_task).start();
    let _ = CronActor::new("10 0 * * * * *", spot_kline_task).start();
    let _ = CronActor::new("10 0 * * * * *", swap_funding_rate_task).start();
    let _ = CronActor::new("10 0 * * * * *", swap_kline_task).start();

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
            let create_sql = table.create_table_statement();
            let table_initial_stmt = create_sql.split(';');
            for stmt in table_initial_stmt {
                let sql = stmt.trim();
                if !sql.is_empty() {
                    conn.execute(sql, [])?;
                }
            }
        }
    }
    Ok(())
}

use crate::actix_jobs::{AsyncRepeatTask, CronActor};
use crate::binance::binance_consts::BinanceTables::SpotKline;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::history_task::{DuckDBHistoryDataWriter, KlinePo, UpdateHistoryTask};
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::{DefaultHistoryFetcherFactory, ExchangeDashBoard};
use actix::Actor;
use duckdb::Connection;
use std::sync::Arc;
use yue::binance::bn_models::BinanceKline;
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

    let spot_info = dashboard.spot_info();
    initial_table()?;
    let kline_fetch_factory: DefaultHistoryFetcherFactory<SimpleHistoryFetcher, KlineParams, BinanceKline> = DefaultHistoryFetcherFactory::new();

    let data_writer = Arc::new(DuckDBHistoryDataWriter::new(DBProvider::default(), SpotKline.table_name()));

    let spot_update = UpdateHistoryTask::<_, _, KlinePo>::new(kline_fetch_factory, spot_info, data_writer);
    spot_update.execute().await?;

    //TODO： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", dashboard, "update exchange info").start();
    let _ = CronActor::new("10 0 * * * * *", spot_update, "fetch spot ").start();
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
    let table_name = SpotKline.table_name();
    if !table_exists(&conn, &table_name)? {
        // 表不存在，执行建表
        conn.execute(SpotKline.create_table_statement().as_str(), [])?;
    }
    Ok(())
}

use crate::actix_jobs::{AsyncRepeatTask, CronActor};
use crate::binance::binance_consts::BinanceTables::SpotKline;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::binance::kline::UpdateKlineTask;
use crate::duck_db::DBProvider;
use crate::errors::MingLuanError;
use crate::exchange::{DefaultKlineFetcherFactory, ExchangeDashBoard};
use actix::Actor;
use yue::binance::spots::SpotKlineFetcher;

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
    //TODO：这段代码，以后移动到数据库初始连接的时候处理
    let conn = DBProvider::default().acquire()?;
    conn.execute(SpotKline.create_table_statement().as_str(), [])?;
    let kline_fetch_factory: DefaultKlineFetcherFactory<SpotKlineFetcher> = DefaultKlineFetcherFactory::new();

    let spot_kline_task = UpdateKlineTask::new(DBProvider::default(), SpotKline.table_name(), kline_fetch_factory, spot_info);
    spot_kline_task.execute().await?;

    //TODO： 更新交易所时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", dashboard, "update exchange info").start();
    let _ = CronActor::new("10 0 * * * * *", spot_kline_task, "fetch spot ").start();
    Ok(())
}

use crate::actix_jobs::CronActor;
use crate::binance::bn_dashboard::BinanceDashboard;
use crate::errors::MingLuanError;
use actix::Actor;

pub fn start_bn_jobs() -> Result<(), MingLuanError> {
    let dashboard = BinanceDashboard::new();
    //每六个小时更新一次。因为这样频率不要那么高
    //TODO： 这里的时间表达式进入Config
    let _ = CronActor::new("30 59 */6 * * * *", dashboard, "update exchange info").start();

    Ok(())
}

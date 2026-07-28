use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio_cron_scheduler::JobScheduler;

// 全局唯一的 scheduler（异步懒加载 + 自动启动）
static SCHEDULER: OnceCell<Arc<JobScheduler>> = OnceCell::const_new();

pub async fn get_scheduler() -> Arc<JobScheduler> {
    SCHEDULER
        .get_or_init(|| async {
            let sched = JobScheduler::new().await.expect("Failed to create JobScheduler");

            sched.start().await.expect("Failed to start JobScheduler");

            Arc::new(sched)
        })
        .await
        .clone()
}

// ====================== 宏定义（支持 uuid + locked） ======================

/// 添加 cron 任务（支持 uuid 和 locked）
/// 用法： cron_job!("*/10 * * * * *", |uuid, locked| { Box::pin(async move { ... }) })】
/// 秒 分 小时 日期(每月） 月 周
#[macro_export]
macro_rules! cron_job {
    ($cron:expr, $closure:expr) => {{
        let sched = $crate::scheduler::get_scheduler().await;
        let job = tokio_cron_scheduler::Job::new_async($cron, $closure).expect("Failed to create cron job");
        sched.add(job).await
    }};
}

/// 添加延迟一次性任务
#[macro_export]
macro_rules! one_shot_job {
    ($delay:expr, $closure:expr) => {{
        let sched = $crate::scheduler::get_scheduler().await;
        let job = tokio_cron_scheduler::Job::new_one_shot_async($delay, $closure).expect("Failed to create one-shot job");
        sched.add(job).await
    }};
}

/// 添加重复执行的任务（每隔固定时间重复一次）
/// 用法： repeated_job!(std::time::Duration::from_secs(5), |uuid, locked| { Box::pin(async move { ... }) })
#[macro_export]
macro_rules! repeated_job {
    ($interval:expr, $closure:expr) => {{
        let sched = $crate::scheduler::get_scheduler().await;
        let job = tokio_cron_scheduler::Job::new_repeated_async($interval, $closure).expect("Failed to create repeated job");
        sched.add(job).await
    }};
}

// 导出宏，方便其他模块直接使用
pub use crate::{cron_job, one_shot_job, repeated_job};

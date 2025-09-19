use crate::errors::MingLuanError;
use actix::{Actor, AsyncContext, Context};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cron::Schedule;
use log::{debug, error, info};
use std::str::FromStr;
use std::time::Duration;

#[async_trait]
pub trait AsyncRepeatTask: Send + Sync + Clone + Unpin + 'static {
    async fn execute(&self) -> Result<(), MingLuanError>;
}

// 泛型定时任务 Actor
pub struct CronActor<T: AsyncRepeatTask> {
    schedule: Schedule,      // cron 调度器
    next_run: DateTime<Utc>, // 下一次执行时间
    task: T,
    task_name: String,
}

impl<T: AsyncRepeatTask> Actor for CronActor<T> {
    type Context = Context<Self>;

    // Actor 启动时初始化定时任务
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("{} started", self.task_name);
        self.schedule_next(ctx); // 调度第一次任务
    }

    // Actor 停止时调用
    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("{} stopped", self.task_name);
    }
}

impl<T: AsyncRepeatTask> CronActor<T> {
    // 创建 Actor 实例
    pub(crate) fn new(cron_expr: &str, task: T, task_name: &str) -> Self {
        let schedule = Schedule::from_str(cron_expr).expect("Invalid cron expression");
        let next_run = schedule.upcoming(Utc).next().expect("No upcoming schedule");
        CronActor {
            schedule,
            next_run,
            task,
            task_name: String::from(task_name),
        }
    }

    // 调度下一次任务
    fn schedule_next(&mut self, ctx: &mut Context<Self>) {
        let now = Utc::now();
        let duration = (self.next_run - now).to_std().unwrap_or(Duration::from_secs(0));
        let task = self.task.clone();
        let task_name = self.task_name.clone();
        ctx.run_later(duration, move |actor, ctx| {
            debug!("{} at {}", actor.task_name, Utc::now().to_rfc3339());
            actix::spawn(async move {
                if let Err(e) = task.execute().await {
                    error!("Error executing {}: {:?}", task_name, e)
                };
            });
            actor.next_run = actor.schedule.upcoming(Utc).next().expect("No upcoming schedule");
            actor.schedule_next(ctx); // 递归调度
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Clone)]
    struct SuccessTask {
        pub called: Arc<Mutex<u32>>,
    }

    #[async_trait]
    impl AsyncRepeatTask for SuccessTask {
        async fn execute(&self) -> Result<(), MingLuanError> {
            let mut count = self.called.lock().unwrap();
            *count += 1;
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FailTask;

    #[async_trait]
    impl AsyncRepeatTask for FailTask {
        async fn execute(&self) -> Result<(), MingLuanError> {
            Err(MingLuanError::CustomError("fail".to_string()))
        }
    }

    #[actix_rt::test]
    async fn schedules_and_executes_success_task() {
        let called = Arc::new(Mutex::new(0));
        let task = SuccessTask { called: called.clone() };
        let _addr = CronActor::new("*/1 * * * * * *", task, "SuccessTask").start();
        actix_rt::time::sleep(Duration::from_secs(2)).await;
        let count = *called.lock().unwrap();
        assert!(count >= 1);
    }

    #[actix_rt::test]
    async fn schedules_and_handles_fail_task() {
        let task = FailTask;
        let mut actor = CronActor::new("*/1 * * * * * *", task, "FailTask");
        let mut ctx = Context::new();
        actor.schedule_next(&mut ctx);
        actix_rt::time::sleep(Duration::from_secs(2)).await;
        // 没有 panic 即为通过
    }

    #[test]
    fn invalid_cron_expression_panics() {
        let task = FailTask;
        let result = std::panic::catch_unwind(|| {
            CronActor::new("invalid cron", task, "InvalidTask");
        });
        assert!(result.is_err());
    }

    #[test]
    fn task_name_is_set_correctly() {
        let task = FailTask;
        let actor = CronActor::new("*/1 * * * * * *", task, "MyTaskName");
        assert_eq!(actor.task_name, "MyTaskName");
    }
}

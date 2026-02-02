use crate::data_integrity::models::ValidationResult;
use crate::errors::YuError;
use actix::prelude::*;
use async_trait::async_trait;
use chrono::Utc;
use cron::Schedule;
use log::{error, info};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// 可插拔校验策略接口，Checker 调用实现校验逻辑。
#[async_trait]
pub trait ValidationStrategy: Send + Sync {
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError>;

    fn name(&self) -> &'static str;
}

#[derive(Clone)]
pub enum ScheduleSpec {
    Interval(u64), // ms
    Cron(String),  // cron expression
}

pub struct CheckActor {
    pub name: String,
    pub strategy: Arc<dyn ValidationStrategy>,
    pub schedule: ScheduleSpec,
    pub timeout_ms: u64,
    pub subscriber: Recipient<ValidationResult>,
    // For cron scheduling (hold join handle so we can abort if needed)
    cron_handle: Option<JoinHandle<()>>,
}

impl CheckActor {
    pub fn new(
        name: impl Into<String>,
        strategy: Arc<dyn ValidationStrategy>,
        schedule: ScheduleSpec,
        timeout_ms: u64,
        subscriber: Recipient<ValidationResult>,
    ) -> Self {
        Self {
            name: name.into(),
            strategy,
            schedule,
            timeout_ms,
            subscriber,
            cron_handle: None,
        }
    }
}

impl Actor for CheckActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(10000);
        // initial run
        let _name = self.name.clone();

        // schedule
        match &self.schedule {
            ScheduleSpec::Interval(ms) => {
                let ms = *ms;
                ctx.run_interval(std::time::Duration::from_millis(ms.max(1)), move |act, _ctx| {
                    let strategy = act.strategy.clone();
                    let subscriber = act.subscriber.clone();
                    let timeout_ms = act.timeout_ms;
                    actix::spawn(async move {
                        info!("开始检测 {}...", strategy.name());
                        let res = run_strategy_with_timeout(strategy, timeout_ms).await;
                        if let Some(r) = res {
                            let _ = subscriber.do_send(r);
                        }
                    });
                });
            }
            ScheduleSpec::Cron(expr) => {
                // parse schedule
                let expr = expr.clone();
                let _name = self.name.clone();
                let strategy = self.strategy.clone();
                let subscriber = self.subscriber.clone();
                let timeout_ms = self.timeout_ms;

                // spawn a background task to handle cron timing
                let handle = actix::spawn(async move {
                    let schedule = Schedule::from_str(&expr).expect("invalid cron expression");
                    let mut upcoming = schedule.upcoming(Utc);
                    loop {
                        if let Some(next) = upcoming.next() {
                            let now = Utc::now();
                            let dur = (next - now).to_std().unwrap_or(StdDuration::from_secs(0));
                            sleep(dur).await;
                            // execute
                            let strategy = strategy.clone();
                            let subscriber = subscriber.clone();
                            let timeout_ms = timeout_ms;
                            actix::spawn(async move {
                                info!("开始检测 {}...", strategy.name());
                                let res = run_strategy_with_timeout(strategy, timeout_ms).await;
                                if let Some(r) = res {
                                    let _ = subscriber.do_send(r);
                                }
                            });
                        } else {
                            break;
                        }
                    }
                });

                // store handle so it can be aborted if actor stops
                self.cron_handle = Some(handle);
            }
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // nothing
    }
}

// 将函数签名改为 pub(crate) ，以便 supervisor 在启动时可调用做一次初始化校验
pub(crate) async fn run_strategy_with_timeout(strategy: Arc<dyn ValidationStrategy>, timeout_ms: u64) -> Option<ValidationResult> {
    let name = strategy.name().to_string();
    let handle = tokio::spawn(async move {
        // wrap in catch_unwind if strategy may panic
        let res = strategy.validate().await;
        res
    });

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), handle).await {
        Ok(join) => match join {
            Ok(res) => match res {
                Ok(r) => r,
                Err(e) => {
                    error!("strategy {} returned error: {:?}", name, e);
                    None
                }
            },
            Err(e) => {
                error!("strategy {} panicked: {:?}", name, e);
                None
            }
        },
        Err(_) => {
            error!("timeout after {} ms for strategy {}", timeout_ms, name);
            None
        }
    }
}

use crate::data_integrity::models::ValidationResult;
use crate::data_integrity::strategy::ValidationStrategy;
use actix::prelude::*;
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use yue::tools::get_snow_flake_id_u64;

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
        // initial run
        let strategy = self.strategy.clone();
        let subscriber = self.subscriber.clone();
        let _name = self.name.clone();
        let timeout_ms = self.timeout_ms;
        actix::spawn(async move {
            let res = run_strategy_with_timeout(strategy, timeout_ms).await;
            let _ = subscriber.do_send(res);
        });

        // schedule
        match &self.schedule {
            ScheduleSpec::Interval(ms) => {
                let ms = *ms;
                ctx.run_interval(std::time::Duration::from_millis(ms.max(1)), move |act, _ctx| {
                    let strategy = act.strategy.clone();
                    let subscriber = act.subscriber.clone();
                    let timeout_ms = act.timeout_ms;
                    actix::spawn(async move {
                        let res = run_strategy_with_timeout(strategy, timeout_ms).await;
                        let _ = subscriber.do_send(res);
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
                                let res = run_strategy_with_timeout(strategy, timeout_ms).await;
                                let _ = subscriber.do_send(res);
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
pub(crate) async fn run_strategy_with_timeout(strategy: Arc<dyn ValidationStrategy>, timeout_ms: u64) -> ValidationResult {
    let name = strategy.name().to_string();
    let handle = tokio::spawn(async move {
        // wrap in catch_unwind if strategy may panic
        let res = strategy.validate().await;
        res
    });

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), handle).await {
        Ok(join) => match join {
            Ok(res) => res,
            Err(e) => ValidationResult {
                id: get_snow_flake_id_u64(),
                strategy: name,
                gaps: Vec::new(),
                retry_count: 0,
                error: Some(format!("panic: {:?}", e)),
            },
        },
        Err(_) => ValidationResult {
            id: get_snow_flake_id_u64(),
            strategy: name,
            gaps: Vec::new(),
            retry_count: 0,
            error: Some("timeout".to_string()),
        },
    }
}

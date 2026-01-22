use crate::errors::YuError;
use actix::{Actor, AsyncContext, Context, Handler, Message as ActixMessage, Recipient};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cron::Schedule;
use log::{debug, error, info};
use std::str::FromStr;
use std::time::Duration;

/// 任务完成事件
#[derive(Debug, Clone)]
pub struct TaskCompletionEvent {
    pub task_name: String,
    pub timestamp: DateTime<Utc>,
    pub result: Result<(), String>,
}

impl ActixMessage for TaskCompletionEvent {
    type Result = ();
}

/// 订阅任务完成事件的消息
#[derive(Debug, Clone)]
pub struct SubscribeTask {
    pub subscriber: Recipient<TaskCompletionEvent>,
}

impl ActixMessage for SubscribeTask {
    type Result = ();
}

#[async_trait]
pub trait AsyncRepeatTask: Send + Sync + Clone + Unpin + 'static {
    async fn execute(&self) -> Result<(), YuError>;

    fn task_name(&self) -> &str;
}

/// 泛型定时任务 Actor
///
/// 设计思路：
/// - 基于 cron 表达式实现可重复执行的定时任务
/// - 使用泛型 T 支持任何实现了 AsyncRepeatTask trait 的任务
/// - 采用订阅-发布模式，任务执行完成后广播事件给所有订阅者
/// - 参考 event_bus.rs 的设计，使用 Recipient 和 ActixMessage 实现松耦合
///
/// 扩展点：
/// 1. 订阅者可以接收任务执行完成事件（成功或失败）
/// 2. 可以通过 SubscribeTask 消息动态注册新的订阅者
/// 3. TaskCompletionEvent 包含任务名称、时间戳和执行结果，便于监控和日志
///
/// 业务规范：
/// - cron 表达式必须合法，否则在创建时会 panic
/// - 任务执行失败不会中断调度，会继续执行下一次
/// - 订阅者使用 do_send 保证消息送达（非阻塞）
/// - 所有订阅者都会收到相同的事件副本
pub struct CronActor<T: AsyncRepeatTask> {
    schedule: Schedule,      // cron 调度器
    next_run: DateTime<Utc>, // 下一次执行时间
    task: T,
    subscribers: Vec<Recipient<TaskCompletionEvent>>, // 订阅者列表
}

impl<T: AsyncRepeatTask> Actor for CronActor<T> {
    type Context = Context<Self>;

    // Actor 启动时初始化定时任务
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("{} started", self.task.task_name());
        self.schedule_next(ctx); // 调度第一次任务
    }

    // Actor 停止时调用
    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("{} stopped", self.task.task_name());
    }
}

impl<T: AsyncRepeatTask> CronActor<T> {
    // 创建 Actor 实例
    pub(crate) fn new(cron_expr: &str, task: T) -> Self {
        let schedule = Schedule::from_str(cron_expr).expect("Invalid cron expression");
        let next_run = schedule.upcoming(Utc).next().expect("No upcoming schedule");
        CronActor {
            schedule,
            next_run,
            task,
            subscribers: Vec::new(),
        }
    }

    // 调度下一次任务
    fn schedule_next(&mut self, ctx: &mut Context<Self>) {
        let now = Utc::now();
        let duration = (self.next_run - now).to_std().unwrap_or(Duration::from_secs(0));
        let task = self.task.clone();
        let task_name = self.task.task_name().to_string();
        let subscribers = self.subscribers.clone();

        ctx.run_later(duration, move |actor, ctx| {
            debug!("{} at {}", task_name, Utc::now().to_rfc3339());
            let task_clone = task.clone();
            let task_name_clone = task_name.clone();
            let subscribers_clone = subscribers.clone();

            actix::spawn(async move {
                let execute_time = Utc::now();
                let result = task_clone.execute().await;

                if let Err(e) = &result {
                    error!("Error executing {}: {:?}", task_name_clone, e);
                }

                // 广播任务完成事件
                let event = TaskCompletionEvent {
                    task_name: task_name_clone.clone(),
                    timestamp: execute_time,
                    result: result.map_err(|e| format!("{:?}", e)),
                };

                for subscriber in &subscribers_clone {
                    subscriber.do_send(event.clone());
                }
            });

            actor.next_run = actor.schedule.upcoming(Utc).next().expect("No upcoming schedule");
            actor.schedule_next(ctx); // 递归调度
        });
    }
}

/// 处理 SubscribeTask 消息
impl<T: AsyncRepeatTask> Handler<SubscribeTask> for CronActor<T> {
    type Result = ();

    fn handle(&mut self, msg: SubscribeTask, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscribers.push(msg.subscriber);
        info!(
            "Subscriber registered for task '{}'. Total subscribers: {}",
            self.task.task_name(),
            self.subscribers.len()
        );
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
        async fn execute(&self) -> Result<(), YuError> {
            let mut count = self.called.lock().unwrap();
            *count += 1;
            Ok(())
        }

        fn task_name(&self) -> &str {
            "unit test success task"
        }
    }

    #[derive(Clone)]
    struct FailTask;

    #[async_trait]
    impl AsyncRepeatTask for FailTask {
        async fn execute(&self) -> Result<(), YuError> {
            Err(YuError::CustomError("fail".to_string()))
        }

        fn task_name(&self) -> &str {
            "unit test fail task"
        }
    }

    // 测试订阅者 Actor
    struct TestSubscriber {
        events: Arc<Mutex<Vec<TaskCompletionEvent>>>,
    }

    impl Actor for TestSubscriber {
        type Context = Context<Self>;
    }

    impl Handler<TaskCompletionEvent> for TestSubscriber {
        type Result = ();

        fn handle(&mut self, event: TaskCompletionEvent, _ctx: &mut Context<Self>) -> Self::Result {
            let mut events = self.events.lock().unwrap();
            events.push(event);
        }
    }

    #[actix_rt::test]
    async fn schedules_and_executes_success_task() {
        let called = Arc::new(Mutex::new(0));
        let task = SuccessTask { called: called.clone() };
        let _addr = CronActor::new("*/1 * * * * * *", task).start();
        actix_rt::time::sleep(Duration::from_secs(2)).await;
        let count = *called.lock().unwrap();
        assert!(count >= 1);
    }

    #[actix_rt::test]
    async fn schedules_and_handles_fail_task() {
        let task = FailTask;
        let _addr = CronActor::new("*/1 * * * * * *", task).start();
        actix_rt::time::sleep(Duration::from_secs(2)).await;
    }

    #[test]
    fn invalid_cron_expression_panics() {
        let task = FailTask;
        let result = std::panic::catch_unwind(|| {
            CronActor::new("invalid cron", task);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_task_completion_event_creation() {
        let event = TaskCompletionEvent {
            task_name: "test_task".to_string(),
            timestamp: Utc::now(),
            result: Ok(()),
        };
        assert_eq!(event.task_name, "test_task");
        assert!(event.result.is_ok());

        let fail_event = TaskCompletionEvent {
            task_name: "fail_task".to_string(),
            timestamp: Utc::now(),
            result: Err("error message".to_string()),
        };
        assert_eq!(fail_event.task_name, "fail_task");
        assert!(fail_event.result.is_err());
    }

    #[actix_rt::test]
    async fn test_subscribe_to_cron_actor() {
        let task = SuccessTask {
            called: Arc::new(Mutex::new(0)),
        };
        let addr = CronActor::new("*/1 * * * * * *", task).start();

        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = TestSubscriber { events: events.clone() }.start();

        addr.do_send(SubscribeTask {
            subscriber: subscriber.recipient(),
        });

        actix_rt::time::sleep(Duration::from_millis(100)).await;
    }

    #[actix_rt::test]
    async fn test_broadcast_success_event() {
        let task = SuccessTask {
            called: Arc::new(Mutex::new(0)),
        };
        let addr = CronActor::new("*/1 * * * * * *", task).start();

        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = TestSubscriber { events: events.clone() }.start();

        addr.do_send(SubscribeTask {
            subscriber: subscriber.recipient(),
        });

        actix_rt::time::sleep(Duration::from_secs(2)).await;

        let received_events = events.lock().unwrap();
        assert!(received_events.len() >= 1);
        assert_eq!(received_events[0].task_name, "unit test success task");
        assert!(received_events[0].result.is_ok());
    }

    #[actix_rt::test]
    async fn test_broadcast_failure_event() {
        let task = FailTask;
        let addr = CronActor::new("*/1 * * * * * *", task).start();

        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = TestSubscriber { events: events.clone() }.start();

        addr.do_send(SubscribeTask {
            subscriber: subscriber.recipient(),
        });

        actix_rt::time::sleep(Duration::from_secs(2)).await;

        let received_events = events.lock().unwrap();
        assert!(received_events.len() >= 1);
        assert_eq!(received_events[0].task_name, "unit test fail task");
        assert!(received_events[0].result.is_err());
    }

    #[actix_rt::test]
    async fn test_multiple_subscribers_receive_events() {
        let task = SuccessTask {
            called: Arc::new(Mutex::new(0)),
        };
        let addr = CronActor::new("*/1 * * * * * *", task).start();

        let events1 = Arc::new(Mutex::new(Vec::new()));
        let subscriber1 = TestSubscriber { events: events1.clone() }.start();

        let events2 = Arc::new(Mutex::new(Vec::new()));
        let subscriber2 = TestSubscriber { events: events2.clone() }.start();

        addr.do_send(SubscribeTask {
            subscriber: subscriber1.recipient(),
        });
        addr.do_send(SubscribeTask {
            subscriber: subscriber2.recipient(),
        });

        actix_rt::time::sleep(Duration::from_secs(2)).await;

        let received_events1 = events1.lock().unwrap();
        let received_events2 = events2.lock().unwrap();

        assert!(received_events1.len() >= 1);
        assert!(received_events2.len() >= 1);
        assert_eq!(received_events1[0].task_name, received_events2[0].task_name);
    }

    #[actix_rt::test]
    async fn test_task_execution_without_subscribers() {
        let called = Arc::new(Mutex::new(0));
        let task = SuccessTask { called: called.clone() };
        let _addr = CronActor::new("*/1 * * * * * *", task).start();

        actix_rt::time::sleep(Duration::from_secs(2)).await;

        let count = *called.lock().unwrap();
        assert!(count >= 1);
    }
}

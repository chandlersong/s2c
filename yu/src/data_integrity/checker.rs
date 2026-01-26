// Checker 负责调度校验策略与产出缺口事件，后续任务补全实现逻辑。
use crate::data_integrity::models::ValidationResult;
use crate::data_integrity::strategy::StrategyRegistry;
use actix::prelude::*;
use tokio::time::{timeout, Duration};

pub struct DataIntegrityChecker {
    pub registry: StrategyRegistry,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub subscriber: Option<Recipient<ValidationResultMsg>>,
}

impl DataIntegrityChecker {
    pub fn new(registry: StrategyRegistry, interval_ms: u64) -> Self {
        Self {
            registry,
            interval_ms,
            timeout_ms: 60_000,
            subscriber: None,
        }
    }

    pub fn with_timeout(registry: StrategyRegistry, interval_ms: u64, timeout_ms: u64) -> Self {
        Self {
            registry,
            interval_ms,
            timeout_ms,
            subscriber: None,
        }
    }

    /// 对所有已注册的策略运行一次校验并收集结果（直接调用，不做超时或异常隔离）
    pub async fn run_once(&self) -> Vec<ValidationResult> {
        let mut results = Vec::new();
        let names = self.registry.list().await;
        for name in names {
            if let Some(strategy) = self.registry.get(&name).await {
                let res = strategy.validate().await;
                results.push(res);
            }
        }
        results
    }
}

/// Message: 包装 ValidationResult 用于 Actor 之间传递
#[derive(Clone, Debug)]
pub struct ValidationResultMsg(pub ValidationResult);

impl Message for ValidationResultMsg {
    type Result = ();
}

/// Message: 订阅 Checker 的校验结果
pub struct Subscribe {
    pub recipient: Recipient<ValidationResultMsg>,
}
impl Message for Subscribe {
    type Result = ();
}

impl Actor for DataIntegrityChecker {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let registry = self.registry.clone();
        let timeout_ms = self.timeout_ms;
        let subscriber = self.subscriber.clone();

        // 立即执行一次（spawn 在 runtime 上执行，不阻塞 Actor）
        let initial_sub = subscriber.clone();
        actix::spawn(async move {
            run_once_and_publish(registry, timeout_ms, initial_sub).await;
        });

        // 周期性调度
        let interval = Duration::from_millis(self.interval_ms.max(1));
        ctx.run_interval(interval, move |act, _ctx| {
            let registry = act.registry.clone();
            let timeout_ms = act.timeout_ms;
            let subscriber = act.subscriber.clone();
            actix::spawn(async move {
                run_once_and_publish(registry, timeout_ms, subscriber).await;
            });
        });
    }
}

impl Handler<Subscribe> for DataIntegrityChecker {
    type Result = ();

    fn handle(&mut self, msg: Subscribe, _ctx: &mut Context<Self>) -> Self::Result {
        self.subscriber = Some(msg.recipient.clone());
        // 订阅者刚注册时，立即触发一次校验并发送结果给该订阅者
        let registry = self.registry.clone();
        let timeout_ms = self.timeout_ms;
        let recipient = msg.recipient;
        actix::spawn(async move {
            run_once_and_publish(registry, timeout_ms, Some(recipient)).await;
        });
    }
}

/// 辅助函数：运行一次所有策略，并把结果发送给 subscriber（若有）
async fn run_once_and_publish(registry: StrategyRegistry, timeout_ms: u64, subscriber: Option<Recipient<ValidationResultMsg>>) {
    let names = registry.list().await;
    for name in names {
        if let Some(strategy) = registry.get(&name).await {
            // 使用 tokio task + timeout 来隔离超时与 panic
            let strat = strategy.clone();
            let name = strat.name().to_string();
            let handle = tokio::spawn(async move {
                // 直接调用策略的 validate
                strat.validate().await
            });

            let res = match timeout(Duration::from_millis(timeout_ms), handle).await {
                Ok(join_res) => match join_res {
                    Ok(valid) => valid,
                    Err(join_err) => ValidationResult {
                        strategy: name.clone(),
                        gaps: Vec::new(),
                        retry_count: 0,
                        error: Some(format!("panic: {:?}", join_err)),
                    },
                },
                Err(_) => {
                    // 超时，尝试取消任务并生成带 error 的结果
                    // 注意：abort 是最佳努力
                    // handle.abort(); // handle 已被 moved into timeout wrapper;无法在这里 abort safely
                    ValidationResult {
                        strategy: name.clone(),
                        gaps: Vec::new(),
                        retry_count: 0,
                        error: Some("timeout".to_string()),
                    }
                }
            };

            if let Some(sub) = subscriber.clone() {
                // 忽略 send 结果（此为 best-effort）
                let _ = sub.do_send(ValidationResultMsg(res.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::models::ValidationGap;
    use crate::data_integrity::strategy::ValidationStrategy;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::sync::Mutex;

    // 测试目的：验证 run_once 在没有超时控制的情况下能收集所有策略的校验结果
    // 设计思路：注册 Noop 与 Gap 两种策略后调用 run_once 并检查返回的结果包含两种策略
    // 扩展点：可在策略并发较多时测试性能与资源占用
    struct NoopStrategy;
    #[async_trait]
    impl ValidationStrategy for NoopStrategy {
        async fn validate(&self) -> ValidationResult {
            ValidationResult::ok("noop")
        }
        fn name(&self) -> &'static str {
            "noop"
        }
    }

    // 测试目的：验证当策略返回缺口时，run_once 能正确收集 gaps
    // 设计思路：实现 GapStrategy 返回包含缺口的 ValidationResult，并在 run_once 后验证 gaps
    // 扩展点：可以测试包含多个 gap 的情形以及 gap 字段序列化一致性
    struct GapStrategy;
    #[async_trait]
    impl ValidationStrategy for GapStrategy {
        async fn validate(&self) -> ValidationResult {
            ValidationResult {
                strategy: "gap".to_string(),
                gaps: vec![ValidationGap::MissingData {
                    symbol: "BTCUSDT".to_string(),
                    trade_type: "SPOT".to_string(),
                    start_time: 1,
                    end_time: 2,
                }],
                retry_count: 0,
                error: None,
            }
        }
        fn name(&self) -> &'static str {
            "gap"
        }
    }

    struct SlowStrategy {
        sleep_ms: u64,
    }
    #[async_trait]
    impl ValidationStrategy for SlowStrategy {
        async fn validate(&self) -> ValidationResult {
            tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
            ValidationResult::ok("slow")
        }
        fn name(&self) -> &'static str {
            "slow"
        }
    }

    struct TestSubscriber {
        pub received: Arc<Mutex<Vec<ValidationResult>>>,
    }

    impl Actor for TestSubscriber {
        type Context = Context<Self>;
    }

    impl Handler<ValidationResultMsg> for TestSubscriber {
        type Result = ();

        fn handle(&mut self, msg: ValidationResultMsg, _ctx: &mut Context<Self>) -> Self::Result {
            let mut guard = self.received.lock().unwrap();
            guard.push(msg.0);
        }
    }

    #[actix_rt::test]
    async fn checker_run_once_collects_strategy_results() {
        let registry = StrategyRegistry::new();
        registry.register(Arc::new(NoopStrategy)).await;
        registry.register(Arc::new(GapStrategy)).await;

        let checker = DataIntegrityChecker::new(registry.clone(), 1000);
        let results = checker.run_once().await;

        // 应包含两个策略的结果，且能区分有缺口/无缺口
        let mut found_noop = false;
        let mut found_gap = false;
        for r in results {
            if r.strategy == "noop" {
                found_noop = true;
                assert!(r.gaps.is_empty());
            }
            if r.strategy == "gap" {
                found_gap = true;
                assert_eq!(r.gaps.len(), 1);
            }
        }
        assert!(found_noop && found_gap, "both strategies should have produced results");
    }

    #[actix_rt::test]
    async fn checker_actor_sends_initial_and_periodic_results() {
        let registry = StrategyRegistry::new();
        registry.register(Arc::new(NoopStrategy)).await;
        registry.register(Arc::new(GapStrategy)).await;

        let received = Arc::new(Mutex::new(Vec::new()));
        let subscriber = TestSubscriber { received: received.clone() }.start();

        let checker = DataIntegrityChecker::with_timeout(registry.clone(), 50, 60_000);
        let addr = checker.start();
        addr.send(Subscribe {
            recipient: subscriber.recipient(),
        })
        .await
        .unwrap();

        // 等待 120ms，应该至少收到初次 + 一次周期
        tokio::time::sleep(Duration::from_millis(120)).await;

        let guard = received.lock().unwrap();
        // 至少两轮 * 2 策略 = 4 条消息
        assert!(guard.len() >= 4, "expected at least 4 messages, got {}", guard.len());
    }

    #[actix_rt::test]
    async fn checker_strategy_timeout_and_error_handling() {
        let registry = StrategyRegistry::new();
        // SlowStrategy sleep 50ms
        registry.register(Arc::new(SlowStrategy { sleep_ms: 50 })).await;

        let received = Arc::new(Mutex::new(Vec::new()));
        let subscriber = TestSubscriber { received: received.clone() }.start();

        // 使用 10ms 的超时用于测试
        let checker = DataIntegrityChecker::with_timeout(registry.clone(), 1000, 10);
        let addr = checker.start();
        addr.send(Subscribe {
            recipient: subscriber.recipient(),
        })
        .await
        .unwrap();

        // 等待 80ms 确保任务运行并超时
        tokio::time::sleep(Duration::from_millis(80)).await;

        let guard = received.lock().unwrap();
        assert_eq!(guard.len(), 1);
        let res = &guard[0];
        assert_eq!(res.strategy, "slow");
        assert!(res.error.is_some(), "expected error due to timeout");
        assert_eq!(res.error.as_deref(), Some("timeout"));
    }
}

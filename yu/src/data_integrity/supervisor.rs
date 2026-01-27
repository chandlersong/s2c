// Supervisor 负责生命周期管理与健康状态维护，后续任务补充 Actor 逻辑。

use crate::config::DataIntegrityConfig;
use crate::data_integrity::models::{HealthSnapshot, HealthState, RepairRequest, ValidationResult};
use crate::data_integrity::repair::{RecordRepairResult, RepairExecutor, RepairStrategy};
use actix::prelude::*;
use log::{error, info};
use std::collections::HashMap;
use std::sync::Arc;

// 新增 CheckActor 的依赖
use crate::data_integrity::check::{run_strategy_with_timeout, CheckActor, ScheduleSpec, ValidationStrategy};

/// DataIntegritySupervisor 管理健康状态、策略注册表，并负责启动 Checker。
pub struct DataIntegritySupervisor {
    config: DataIntegrityConfig,
    check_strategy: HashMap<String, Arc<dyn ValidationStrategy>>,
    repair_strategies: HashMap<String, Arc<dyn RepairStrategy>>,
    health: HealthSnapshot,
    repair_job_ids: Vec<u64>,
    repair_recipient: Option<Recipient<RepairRequest>>,
}

impl DataIntegritySupervisor {
    ///
    /// 1. 创建一个RepairExecutor，启动，并且获取其Recipient
    /// 2. new_with_config 不再做初始化校验，初始化逻辑将在 Actor::started 中完成（便于 await 行为由 Actor 启动控制）
    pub async fn new_with_config(
        config: DataIntegrityConfig,
        check_strategy: HashMap<String, Arc<dyn ValidationStrategy>>,
        repair_strategy: HashMap<String, Arc<dyn RepairStrategy>>,
    ) -> Self {
        // 如果 repair_recipient 为 None，则创建一个新的 RepairExecutor

        Self {
            config,
            check_strategy,
            repair_strategies: repair_strategy,
            health: HealthSnapshot::new(HealthState::OK, None),
            repair_job_ids: Vec::new(),
            repair_recipient: None,
        }
    }
}

impl Actor for DataIntegritySupervisor {
    type Context = Context<Self>;

    /// 1.把系统的状态设置为 INITIAL
    /// 2.根据check_registry，完成初始化校验。调用run_strategy_with_timeout
    /// 3.根据check_registry，创建并启动所有的 Checker Actor
    /// 4.自己开始监听每个check Actor的ValidationResult事件。
    /// 5.等到以上完成。休息1s，如果repair_job_ids为0，则为OK。否则改成RECOVERING
    fn started(&mut self, ctx: &mut Self::Context) {
        let repair_job = RepairExecutor::new(ctx.address().recipient::<RecordRepairResult>(), self.repair_strategies.clone()).start();
        self.repair_recipient = Some(repair_job.recipient());

        let supervisor_recipient = ctx.address().recipient::<ValidationResult>();
        let config = self.config.clone();
        let self_addr = ctx.address();
        let check_strategies = self.check_strategy.clone();
        // spawn 异步任务只负责启动周期性 CheckActor（不再执行初始化校验）
        actix::spawn(async move {
            let _initial = HealthSnapshot::new(HealthState::INITIAL, None);

            // 检查每个策略的修复任务是否仍然存在，更新健康状态
            for (name, strategy) in &check_strategies {
                let timeout_ms = config.startup_check_timeout_ms;
                info!("initial data {}...", name);
                let res = run_strategy_with_timeout(strategy.clone(), timeout_ms).await;
                supervisor_recipient.do_send(res);
            }
            // 更新 Supervisor 的健康状态，并等待处理完成，确保状态已应用
            if let Ok((success, reason)) = self_addr.send(SetHealth(HealthState::OK, None)).await {
                if !success {
                    info!("DataIntegritySupervisor initialization detected pending repairs: {:?}", reason);
                }
            } else {
                error!("DataIntegritySupervisor failed to set health state");
            }

            for (name, strategy) in &check_strategies {
                info!("启动 Checker {}", name);
                let timeout_ms = config.startup_check_timeout_ms;
                let schedule = ScheduleSpec::Cron(config.periodic_check_interval_cron.clone());
                let check = CheckActor::new(name.clone(), strategy.clone(), schedule, timeout_ms, supervisor_recipient.clone());
                check.start();
            }
            // 所有 Checker 启动完成后，先标记为 INITIAL（启动中），然后等待 1 秒钟以让初检完成
        });
    }
}

/// 新增内部消息：设置 health
/// 返回值为Bool，String，如果设置成功，就会为True，None，否则时理由
pub struct SetHealth(pub HealthState, pub Option<String>);
impl Message for SetHealth {
    // 修改返回类型为 (bool, Option<String>)
    type Result = (bool, Option<String>);
}
impl Handler<SetHealth> for DataIntegritySupervisor {
    // 使用 MessageResult 包装返回值以满足 actix 的类型约束
    type Result = MessageResult<SetHealth>;

    /// 检测repair_job_ids，如果不为空则设置为RECOVERING
    /// 负责按照他设置的进行设置。
    fn handle(&mut self, msg: SetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        let SetHealth(state, reason) = msg;
        let res = if !self.repair_job_ids.is_empty() {
            let r = reason.clone().or_else(|| Some("repair jobs is running".to_string()));
            self.health = HealthSnapshot::new(HealthState::RECOVERING, r.clone());
            (false, r)
        } else {
            self.health = HealthSnapshot::new(state, reason.clone());
            (true, None)
        };
        MessageResult(res)
    }
}

impl Handler<ValidationResult> for DataIntegritySupervisor {
    type Result = ();

    ///
    /// 判断逻辑。
    /// 1. 如果gaps为空，不做任何操作。
    /// 2. 不为空，做以下操作。
    ///    1. 把health设置为RECOVERING，reason为error。
    ///    2. 把id加入repair_job_ids
    ///    3. 把ValidationResult转换RepairRequest，发送给RepairExecutor，保证相同的id
    ///
    fn handle(&mut self, msg: ValidationResult, _ctx: &mut Self::Context) -> Self::Result {
        if !msg.gaps.is_empty() {
            self.health = HealthSnapshot::new(HealthState::RECOVERING, msg.error.clone());
            self.repair_job_ids.push(msg.id);

            // 构造 RepairRequest 并发送给 RepairExecutor，保持相同的 id
            let req = RepairRequest {
                id: msg.id,
                strategy: msg.strategy.clone(),
                gaps: msg.gaps.clone(),
            };

            // 通过 Recipient 发送 RepairRequest，如果发送失败则记录错误
            if let Some(r) = &self.repair_recipient {
                r.do_send(req);
            } else {
                error!("RepairExecutor recipient is not set in DataIntegritySupervisor");
            }
        }
    }
}

/// Message: 获取当前 HealthSnapshot（只读）
pub struct GetHealthState;

impl Message for GetHealthState {
    type Result = HealthSnapshot;
}

impl Handler<GetHealthState> for DataIntegritySupervisor {
    // 使用 MessageResult 包装返回值以满足 actix 的类型约束
    type Result = MessageResult<GetHealthState>;

    fn handle(&mut self, _msg: GetHealthState, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.health.clone())
    }
}
/// Message: RepairExecutor 上报的修复结果包装，用于 Supervisor 更新状态

impl Handler<RecordRepairResult> for DataIntegritySupervisor {
    type Result = ();

    ///
    /// 判断逻辑。
    /// 1. 如果成功。删除repair_job_ids的对应id
    /// 2. 判断repair_job_ids是否为空。
    /// 2. 失败的话，临时复用repair_job_ids的逻辑
    ///
    /// TODO: 如果失败这里最好通知人来处理。所以暂时先不管具体操作
    ///
    fn handle(&mut self, msg: RecordRepairResult, _ctx: &mut Context<Self>) -> Self::Result {
        let r = msg.0;
        self.repair_job_ids.retain(|&id| id != r.request_id);
        if self.repair_job_ids.is_empty() {
            // 没有待修复任务，恢复为 OK
            self.health = HealthSnapshot::new(HealthState::OK, None);
        } else {
            // 仍有待修复任务，保持 RECOVERING
            self.health = HealthSnapshot::new(HealthState::RECOVERING, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::check::ValidationStrategy;
    use crate::data_integrity::models::{RepairResult, RepairStatus, ValidationGap};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct GapCheckStrategy;

    #[async_trait]
    impl ValidationStrategy for GapCheckStrategy {
        async fn validate(&self) -> ValidationResult {
            ValidationResult {
                id: yue::tools::get_snow_flake_id_u64(),
                strategy: "gap".to_string(),
                gaps: vec![ValidationGap::MissingData {
                    symbol: "BTCUSDT".to_string(),
                    trade_type: "SPOT".to_string(),
                    start_time: 1,
                    end_time: 2,
                    table: "".to_string(),
                }],
                retry_count: 0,
                error: Some("missing data".to_string()),
            }
        }

        fn name(&self) -> &'static str {
            "gap"
        }
    }

    struct NoopCheckStrategy;

    #[async_trait]
    impl ValidationStrategy for NoopCheckStrategy {
        async fn validate(&self) -> ValidationResult {
            ValidationResult::ok("noop")
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    struct DelayRepairStrategy(&'static str);

    #[async_trait]
    impl RepairStrategy for DelayRepairStrategy {
        async fn repair(&self, _: RepairRequest) -> Result<(), String> {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            Ok(())
        }

        fn name(&self) -> &'static str {
            self.0
        }
    }

    #[actix::test]
    async fn supervisor_starts_checkers_and_handles_validation() {
        let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
        check_strategies.insert("gap".to_string(), Arc::new(GapCheckStrategy));

        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, HashMap::new()).await;
        let addr = supervisor.start();

        // 等待 CheckActor 的初次运行完成（稍微宽裕点）
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // 获取 health 状态，应为 RECOVERING（因为 GapStrategy 返回了 gaps）
        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::RECOVERING);

        // 由于 Handler 会记录 repair_job_ids，检查至少记录了一个 id
        // 通过请求 Addr 的内部状态不容易直接读取，因此我们发送一个 ValidationResult 空缺来触发不带 gap 的更新，
        // 再检查 health 是否变为 OK；但我们也可以通过发送带 gap 的 ValidationResult 来触发 repair_job_ids 累积。

        // 再发送一个带 gap 的 ValidationResult 并等待
        let vr = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: "gap2".to_string(),
            gaps: vec![ValidationGap::MissingData {
                symbol: "ETHUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 1,
                end_time: 2,
                table: "".to_string(),
            }],
            retry_count: 0,
            error: Some("missing data 2".to_string()),
        };

        addr.do_send(vr.clone());
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let health2 = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health2.state, HealthState::RECOVERING);
    }

    #[actix::test]
    async fn supervisor_initialization_ok_when_no_gaps() {
        let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
        check_strategies.insert("noop".to_string(), Arc::new(NoopCheckStrategy));

        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, HashMap::new()).await;
        let addr = supervisor.start();

        // 初始化完成后，health 应为 OK
        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::OK);
    }

    #[actix::test]
    async fn set_health_success_when_no_repairs() {
        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, HashMap::new(), HashMap::new()).await;
        let addr = supervisor.start();
        //等待完成初始化
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // 尝试设置为 DEGRADED，并带有错误信息
        let res = addr.send(SetHealth(HealthState::DEGRADED, Some("manual".to_string()))).await.unwrap();
        assert!(res.0);
        assert!(res.1.is_none());

        // 确认内部 health 被设置为 DEGRADED 并包含我们提供的错误信息
        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::DEGRADED);
        assert_eq!(health.reason.as_deref(), Some("manual"));
    }

    #[actix::test]
    async fn set_health_forced_recover_when_repairs_pending() {
        let mut check_strategies: HashMap<String, Arc<dyn ValidationStrategy>> = HashMap::new();
        check_strategies.insert("gap".to_string(), Arc::new(GapCheckStrategy));
        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, check_strategies, HashMap::new()).await;
        let addr = supervisor.start();

        // 触发一个带 gap 的 ValidationResult，导致 repair_job_ids 记录
        let vr = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: "test-gap".to_string(),
            gaps: vec![ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 1,
                end_time: 2,
                table: "".to_string(),
            }],
            retry_count: 0,
            error: Some("missing".to_string()),
        };

        addr.do_send(vr);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 现在尝试设置为 OK，但应该被强制为 RECOVERING
        let res = addr.send(SetHealth(HealthState::OK, None)).await.unwrap();
        assert!(!res.0);
        assert_eq!(res.1.unwrap_or_default(), "repair jobs pending".to_string());

        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::RECOVERING);
        assert_eq!(health.reason.as_deref(), Some("repair jobs pending"));
    }

    #[actix::test]
    async fn handle_validation_result_no_gaps() {
        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, HashMap::new(), HashMap::new()).await;
        let addr = supervisor.start();

        // 初始 health 为 OK
        let initial_health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(initial_health.state, HealthState::OK);

        // 发送没有 gaps 的 ValidationResult
        let vr = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: "test".to_string(),
            gaps: vec![],
            retry_count: 0,
            error: None,
        };
        addr.do_send(vr);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // health 应该不变，仍为 OK
        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::OK);
    }

    #[actix::test]
    async fn handle_validation_result_with_gaps() {
        let delay_repair_strategy = DelayRepairStrategy("test-gap");
        let mut repair_strategies: HashMap<String, Arc<dyn RepairStrategy>> = HashMap::new();
        repair_strategies.insert("test-gap".to_string(), Arc::new(delay_repair_strategy));
        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, HashMap::new(), repair_strategies).await;
        let addr = supervisor.start();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // 发送有 gaps 的 ValidationResult
        let vr = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: "test-gap".to_string(),
            gaps: vec![ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 1,
                end_time: 2,
                table: "".to_string(),
            }],
            retry_count: 0,
            error: Some("missing data".to_string()),
        };
        addr.do_send(vr.clone());

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // health 应该为 RECOVERING，reason 为 error
        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::RECOVERING);
        assert_eq!(health.reason.as_deref(), Some("missing data"));
    }

    #[actix::test]
    async fn repair_result_succeeded_removes_id_and_sets_ok() {
        let config = DataIntegrityConfig::default();
        let supervisor = DataIntegritySupervisor::new_with_config(config, HashMap::new(), HashMap::new()).await;
        let addr = supervisor.start();

        // 先触发一个带 gap 的 ValidationResult，添加到 repair_job_ids
        let vr = ValidationResult {
            id: yue::tools::get_snow_flake_id_u64(),
            strategy: "test-gap".to_string(),
            gaps: vec![ValidationGap::MissingData {
                symbol: "BTCUSDT".to_string(),
                trade_type: "SPOT".to_string(),
                start_time: 1,
                end_time: 2,
                table: "".to_string(),
            }],
            retry_count: 0,
            error: Some("missing data".to_string()),
        };
        let id = vr.id;
        addr.do_send(vr);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // 发送修复成功的结果
        let rr = RepairResult {
            request_id: id,
            strategy: "test-gap".to_string(),
            status: RepairStatus::SUCCEEDED,
            error: None,
        };
        addr.do_send(RecordRepairResult(rr));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let health = addr.send(GetHealthState).await.unwrap();
        assert_eq!(health.state, HealthState::OK);
    }
}

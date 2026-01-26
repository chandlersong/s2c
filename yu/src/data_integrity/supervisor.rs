// Supervisor 负责生命周期管理与健康状态维护，后续任务补充 Actor 逻辑。

use crate::config::DataIntegrityConfig;
use crate::data_integrity::checker::DataIntegrityChecker;
use crate::data_integrity::models::{HealthSnapshot, HealthState};
use crate::data_integrity::repair::RepairExecutor;
use crate::data_integrity::strategy::StrategyRegistry;
use actix::prelude::*;

/// DataIntegritySupervisor 管理健康状态、策略注册表，并负责启动 Checker。
pub struct DataIntegritySupervisor {
    pub config: DataIntegrityConfig,
    pub registry: StrategyRegistry,
    pub health: HealthSnapshot,
    pub checker: Option<Addr<DataIntegrityChecker>>,
    pub repair_executor: Option<Addr<RepairExecutor>>,
}

impl DataIntegritySupervisor {
    pub fn new_with_config(config: DataIntegrityConfig) -> Self {
        Self {
            config,
            registry: StrategyRegistry::new(),
            health: HealthSnapshot::new(HealthState::OK, None),
            checker: None,
            repair_executor: None,
        }
    }

    pub fn start_with_config(config: DataIntegrityConfig) -> Addr<Self> {
        DataIntegritySupervisor::new_with_config(config).start()
    }
}

impl Actor for DataIntegritySupervisor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        // 启动 RepairExecutor
        let repair = RepairExecutor::default().start();
        // 启动真实的 DataIntegrityChecker，并订阅 RepairExecutor
        let interval_ms = 60_000; // 默认周期 60s（可后续基于 cron 解析）
        let checker = DataIntegrityChecker::with_timeout(self.registry.clone(), interval_ms, self.config.startup_check_timeout_ms);
        let checker_addr = checker.start();
        // 将 RepairExecutor 订阅到 Checker（使用 clone 获取 recipient，避免 move）
        let _ = checker_addr.do_send(crate::data_integrity::checker::Subscribe {
            recipient: repair.clone().recipient(),
        });

        self.checker = Some(checker_addr);
        self.repair_executor = Some(repair);
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

/// Message: 获取当前 DataIntegrityConfig（只读）
pub struct GetDataIntegrityConfig;
impl Message for GetDataIntegrityConfig {
    type Result = DataIntegrityConfig;
}

impl Handler<GetDataIntegrityConfig> for DataIntegritySupervisor {
    type Result = MessageResult<GetDataIntegrityConfig>;

    fn handle(&mut self, _msg: GetDataIntegrityConfig, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.config.clone())
    }
}

/// Message: 查询 Checker 是否已启动
pub struct IsCheckerRunning;
impl Message for IsCheckerRunning {
    type Result = bool;
}

impl Handler<IsCheckerRunning> for DataIntegritySupervisor {
    type Result = MessageResult<IsCheckerRunning>;

    fn handle(&mut self, _msg: IsCheckerRunning, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.checker.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RepairBackoffConfig;

    // 测试目的：验证 Supervisor 启动后默认健康状态为 OK
    // 设计思路：使用默认配置启动 Supervisor，并发送 GetHealthState 验证返回值
    // 扩展点：可以在后续添加 Supervisor 状态变更与告警触发的测试
    #[actix_rt::test]
    async fn supervisor_initial_state_is_ok() {
        let cfg = DataIntegrityConfig::default();
        let addr = DataIntegritySupervisor::start_with_config(cfg);

        let health = addr.send(GetHealthState).await.expect("actor mailbox closed");
        assert_eq!(health.state, HealthState::OK);
    }

    // 测试目的：验证 Supervisor 能正确返回注入的配置
    // 设计思路：构造自定义 DataIntegrityConfig 并验证 GetDataIntegrityConfig 返回一致
    // 扩展点：测试更多配置字段与边界值
    #[actix_rt::test]
    async fn supervisor_reads_config() {
        let cfg = DataIntegrityConfig {
            startup_check_timeout_ms: 12_345,
            periodic_check_interval_cron: "0 * * * * * *".to_string(),
            repair_backoff: RepairBackoffConfig { max_retries: 7 },
        };

        let addr = DataIntegritySupervisor::start_with_config(cfg.clone());
        let read_cfg = addr.send(GetDataIntegrityConfig).await.expect("actor mailbox closed");
        assert_eq!(read_cfg.startup_check_timeout_ms, 12_345);
        assert_eq!(read_cfg.repair_backoff.max_retries, 7);
    }

    // 测试目的：验证 Supervisor 启动时能拉起 Checker（并保存地址）
    // 设计思路：启动 Supervisor 并通过 IsCheckerRunning 查询 Checker 是否存在
    // 扩展点：可以验证 Checker 与 RepairExecutor 的订阅关系以及健康状态随修复结果变化
    #[actix_rt::test]
    async fn supervisor_spawns_checker() {
        let cfg = DataIntegrityConfig::default();
        let addr = DataIntegritySupervisor::start_with_config(cfg);
        let running = addr.send(IsCheckerRunning).await.expect("actor mailbox closed");
        assert!(running, "Checker should be running after supervisor started");
    }
}

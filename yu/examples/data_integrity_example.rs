use actix::Actor;
use actix_rt::main;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use yu::config::DataIntegrityConfig;
use yu::data_integrity::models::ValidationResult;
use yu::data_integrity::strategy::ValidationStrategy;
use yu::data_integrity::supervisor::{DataIntegritySupervisor, GetHealthState, IsCheckerRunning};

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

#[main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 使用默认 DataIntegrityConfig 创建 Supervisor 实例
    let cfg = DataIntegrityConfig::default();
    let supervisor = DataIntegritySupervisor::new_with_config(cfg.clone());

    // 在启动之前将策略注册到 supervisor 内部的 StrategyRegistry
    supervisor.registry.register(Arc::new(NoopStrategy)).await;

    // 启动 Supervisor actor
    let sup_addr = supervisor.start();

    // 查询初始健康状态
    let health = sup_addr.send(GetHealthState).await?;
    println!("Supervisor initial health: {:?}", health);

    // 查询 Checker 是否已启动
    let running = sup_addr.send(IsCheckerRunning).await?;
    println!("Is checker running: {}", running);

    // 等待一段时间让内部的 Checker 执行一次初始校验并交付给内部 RepairExecutor
    sleep(Duration::from_millis(2_500)).await;

    println!("Supervisor example finished.");
    Ok(())
}

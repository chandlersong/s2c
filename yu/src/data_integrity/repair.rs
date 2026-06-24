// RepairExecutor 负责接收校验结果并调度修复请求（简化实现）
use crate::data_integrity::models::RepairRequest;
use async_trait::async_trait;
use yue::errors::YueError;

/// Repair 策略接口：实现具体修复逻辑。返回 Ok(()) 表示成功，Err(reason) 表示失败（会触发重试/上报）。
#[async_trait]
pub trait RepairStrategyTrait: Send + Sync + 'static {
    async fn repair(&self, req: RepairRequest) -> Result<(), YueError>;
    fn name(&self) -> &'static str; // optional name
}

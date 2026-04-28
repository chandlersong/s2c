use crate::data_integrity::models::ValidationResult;
use crate::errors::YuError;
use async_trait::async_trait;

/// 可插拔校验策略接口，Checker 调用实现校验逻辑。
#[async_trait]
pub trait ValidationStrategyTrait: Send + Sync {
    async fn validate(&self) -> Result<Option<ValidationResult>, YuError>;

    fn name(&self) -> String;
}

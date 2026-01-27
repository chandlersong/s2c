// 使用新的 Supervisor + CheckActor 设计的示例

use actix_rt::main;
use async_trait::async_trait;

use yu::data_integrity::models::ValidationResult;

use yu::data_integrity::strategy::ValidationStrategy;

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

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

use crate::data_integrity::models::ValidationResult;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 可插拔校验策略接口，Checker 调用实现校验逻辑。
#[async_trait]
pub trait ValidationStrategy: Send + Sync {
    async fn validate(&self) -> ValidationResult;

    fn name(&self) -> &'static str;
}

/// 策略注册表：按名称管理策略实例，便于运行时查找与替换。
#[derive(Default, Clone)]
pub struct StrategyRegistry {
    strategies: Arc<RwLock<HashMap<String, Arc<dyn ValidationStrategy>>>>,
}

impl StrategyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, strategy: Arc<dyn ValidationStrategy>) {
        let mut guard = self.strategies.write().await;
        guard.insert(strategy.name().to_string(), strategy);
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn ValidationStrategy>> {
        let guard = self.strategies.read().await;
        guard.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<String> {
        let guard = self.strategies.read().await;
        guard.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_integrity::models::ValidationGap;

    // 测试目的：验证 Registry 能注册并返回策略实例，且策略按 name 可被调用
    // 设计思路：实现一个 NoopStrategy 并注册，调用 validate 验证返回值
    // 扩展点：可加入并发注册/读取与替换策略的测试
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

    // 测试目的：验证 Registry 列出所有已注册策略并能返回带缺口的策略结果
    // 设计思路：注册 Noop 与 Gap 两个策略，验证 list 排序后包含两者，并调用 gap 策略产生 gap
    // 扩展点：可以测试 list 的一致性以及高并发下的性能
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

    #[tokio::test]
    async fn registry_register_and_get() {
        let registry = StrategyRegistry::new();
        let noop = Arc::new(NoopStrategy);
        registry.register(noop.clone()).await;

        let handle = registry.get("noop").await.unwrap();
        let result = handle.validate().await;

        assert_eq!(result.strategy, "noop");
        assert!(result.gaps.is_empty());
    }

    #[tokio::test]
    async fn registry_list_and_gap_strategy() {
        let registry = StrategyRegistry::new();
        registry.register(Arc::new(NoopStrategy)).await;
        registry.register(Arc::new(GapStrategy)).await;

        let mut names = registry.list().await;
        names.sort();
        assert_eq!(names, vec!["gap".to_string(), "noop".to_string()]);

        let gap = registry.get("gap").await.unwrap();
        let result = gap.validate().await;
        assert_eq!(result.strategy, "gap");
        assert_eq!(result.gaps.len(), 1);
    }
}

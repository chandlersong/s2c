use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// 全局健康状态枚举，代表当前数据完整性的总体态势。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthState {
    OK,
    DEGRADED,
    RECOVERING,
    FAILED,
}

/// 状态快照，附带时间戳与可选原因，供 Supervisor 对外查询使用。
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub state: HealthState,
    pub updated_at_ms: u128,
    pub reason: Option<String>,
}

impl HealthSnapshot {
    pub fn new(state: HealthState, reason: Option<String>) -> Self {
        let updated_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
        Self {
            state,
            updated_at_ms,
            reason,
        }
    }
}

/// 校验缺口类型：当前仅支持缺失数据窗口。
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationGap {
    MissingData {
        symbol: String,
        trade_type: String,
        start_time: u64,
        end_time: u64,
    },
}

/// 校验结果事件，Checker 产出，交由 RepairExecutor 消费。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub strategy: String,
    #[serde(default)]
    pub gaps: Vec<ValidationGap>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub error: Option<String>,
}

impl ValidationResult {
    pub fn ok(strategy: impl Into<String>) -> Self {
        Self {
            strategy: strategy.into(),
            gaps: Vec::new(),
            retry_count: 0,
            error: None,
        }
    }
}

/// 修复请求，通常由 ValidationResult 转化后放入 RepairExecutor。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRequest {
    pub id: String,
    pub strategy: String,
    #[serde(default)]
    pub gaps: Vec<ValidationGap>,
}

/// 修复状态。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RepairStatus {
    SUCCEEDED,
    FAILED,
    SKIPPED,
}

/// 修复结果，反馈给 Supervisor 更新健康状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairResult {
    pub request_id: String,
    pub strategy: String,
    pub status: RepairStatus,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试目的：验证 HealthState 序列化/反序列化的行为，确保枚举按 UPPERCASE 序列化
    // 设计思路：对 HealthState::DEGRADED 做 serde 的 round-trip
    // 扩展点：可增加对其他枚举值及错误字符串反序列化的边界测试
    #[test]
    fn health_state_serde_round_trip() {
        let json = serde_json::to_string(&HealthState::DEGRADED).unwrap();
        assert_eq!(json, "\"DEGRADED\"");

        let state: HealthState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, HealthState::DEGRADED);
    }

    // 测试目的：验证 ValidationGap 与 ValidationResult 的序列化与反序列化正确性
    // 设计思路：构造带缺口的 ValidationResult，序列化再反序列化后比对字段一致性
    // 扩展点：可以增加包含多个 gap、error 字段以及 retry_count 的组合场景
    #[test]
    fn validation_gap_enum_and_result_serialization() {
        let gap = ValidationGap::MissingData {
            symbol: "BTCUSDT".to_string(),
            trade_type: "SPOT".to_string(),
            start_time: 1_700_000_000_000,
            end_time: 1_700_000_100_000,
        };

        let result = ValidationResult {
            strategy: "noop".to_string(),
            gaps: vec![gap.clone()],
            retry_count: 1,
            error: Some("timeout".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();
        let decoded: ValidationResult = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.strategy, "noop");
        assert_eq!(decoded.gaps, vec![gap]);
        assert_eq!(decoded.retry_count, 1);
        assert_eq!(decoded.error.as_deref(), Some("timeout"));
    }
}

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use yue::tools::get_snow_flake_id_u64;

/// 全局健康状态枚举，代表当前数据完整性的总体态势。
/// 关于状态管理。
/// 1. 启动时候检查，默认是FAILED
/// 2. 定期的检查，除非发现问题，否则为OK
/// 3. 发现问题后，状态变为FAILED
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HealthState {
    OK,
    DEGRADED,
    RECOVERING,
    FAILED,
    INITIAL,
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
        table: String,
    },
    UNKnowError {
        //占位
        reason: String,
    },
}

/// 校验结果事件，Checker 产出，交由 RepairExecutor 消费。
///
/// ValidationResult,
///
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub id: u64,
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
            id: get_snow_flake_id_u64(),
            strategy: strategy.into(),
            gaps: Vec::new(),
            retry_count: 0,
            error: None,
        }
    }
}

impl Message for ValidationResult {
    type Result = ();
}

/// 修复请求，通常由 ValidationResult 转化后放入 RepairExecutor。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairRequest {
    pub id: u64,
    pub strategy: String,
    #[serde(default)]
    pub gaps: Vec<ValidationGap>,
}

// 为 RepairRequest 实现 actix Message trait，以便可以通过 Recipient 发送
impl actix::prelude::Message for RepairRequest {
    type Result = ();
}

/// FUTURE: 后面这些在以后关于数据库状态使用比较好。
/// 修复状态。
/// 我觉得这里有点过度设计。感觉有一个两个状态就够了。现阶段先保留SKIPPED
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
    pub request_id: u64, // 对应的 ValidationResult ID
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
            table: "".to_string(),
        };

        let result = ValidationResult {
            id: 0,
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

use crate::errors::LiError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// 任务完成事件
#[derive(Debug, Clone)]
pub struct TaskCompletionEvent {
    pub task_name: String,
    pub timestamp: DateTime<Utc>,
    pub result: Result<(), String>,
}

#[async_trait]
pub trait AsyncRepeatTask: Send + Sync + Clone + Unpin + 'static {
    /**
     * 专门用于初始化。
     */
    async fn initial_data(&self) -> Result<(), LiError>;

    /**
     * 专门用于日常更新。
     */
    async fn execute(&self) -> Result<(), LiError>;
    fn task_name(&self) -> &str;
}

/// 处理 SubscribeEvent<TaskCompletionEvent> 消息

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_completion_event_creation() {
        let event = TaskCompletionEvent {
            task_name: "test_task".to_string(),
            timestamp: Utc::now(),
            result: Ok(()),
        };
        assert_eq!(event.task_name, "test_task");
        assert!(event.result.is_ok());

        let fail_event = TaskCompletionEvent {
            task_name: "fail_task".to_string(),
            timestamp: Utc::now(),
            result: Err("error message".to_string()),
        };
        assert_eq!(fail_event.task_name, "fail_task");
        assert!(fail_event.result.is_err());
    }
}

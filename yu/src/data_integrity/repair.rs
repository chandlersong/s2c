// RepairExecutor 负责接收校验结果并调度修复请求（简化实现）
use crate::data_integrity::models::{RepairRequest, RepairResult, RepairStatus};
use actix::prelude::*;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Repair 策略接口：实现具体修复逻辑。返回 Ok(()) 表示成功，Err(reason) 表示失败（会触发重试/上报）。
#[async_trait]
pub trait RepairStrategy: Send + Sync + 'static {
    async fn repair(&self, req: RepairRequest) -> Result<(), String>;
    fn name(&self) -> &'static str; // optional name
}

/// Message: 注册一个 repair 策略到 RepairExecutor
pub struct RegisterRepairStrategy(pub String, pub Arc<dyn RepairStrategy>);
impl Message for RegisterRepairStrategy {
    type Result = ();
}

pub struct RepairExecutor {
    // 简单队列，持有待处理的请求（此处仅演示转换与存储）
    pub pending: VecDeque<RepairRequest>,
    pub results: Vec<RepairResult>,
    pub repair_strategies: HashMap<String, Arc<dyn RepairStrategy>>,
}

impl Default for RepairExecutor {
    fn default() -> Self {
        RepairExecutor {
            pending: VecDeque::new(),
            results: Vec::new(),
            repair_strategies: Default::default(),
        }
    }
}

impl Actor for RepairExecutor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        _ctx.set_mailbox_capacity(1000);
    }
}

// 处理来自 Supervisor 的 RepairRequest：将请求入队，记录并返回
impl Handler<RepairRequest> for RepairExecutor {
    type Result = ();

    fn handle(&mut self, msg: RepairRequest, _ctx: &mut Self::Context) -> Self::Result {
        // 简化逻辑：将请求加入待处理队列，并记录一个 SKIPPED 结果占位
        self.pending.push_back(msg.clone());

        // 记录一个占位 RepairResult（实际实现会调用策略并上报结果）
        let res = RepairResult {
            request_id: msg.id,
            strategy: msg.strategy.clone(),
            status: RepairStatus::SKIPPED,
            error: None,
        };
        self.results.push(res);
    }
}

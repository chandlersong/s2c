// RepairExecutor 负责接收校验结果并调度修复请求（简化实现）
use crate::data_integrity::models::{RepairRequest, RepairResult, RepairStatus};
use actix::prelude::*;
use async_trait::async_trait;
use log::{error, info};
use std::collections::HashMap;
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

// 新增：当策略执行完成时，向 RepairExecutor 上报结果
pub struct RecordRepairResult(pub RepairResult);
impl Message for RecordRepairResult {
    type Result = ();
}

pub struct RepairExecutor {
    // 简单队列，持有待处理的请求（此处仅演示转换与存储）
    pub repair_strategies: HashMap<String, Arc<dyn RepairStrategy>>,
    pub report_recipient: Recipient<RecordRepairResult>,
}

impl RepairExecutor {
    pub(crate) fn new(report_recipient: Recipient<RecordRepairResult>, repair_strategies: HashMap<String, Arc<dyn RepairStrategy>>) -> Self {
        RepairExecutor {
            repair_strategies,
            report_recipient,
        }
    }
}

impl Actor for RepairExecutor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        _ctx.set_mailbox_capacity(10000);
    }
}

impl Handler<RegisterRepairStrategy> for RepairExecutor {
    type Result = ();

    fn handle(&mut self, msg: RegisterRepairStrategy, _ctx: &mut Self::Context) -> Self::Result {
        let RegisterRepairStrategy(name, strategy) = msg;
        info!("Registering repair strategy {}", name);
        self.repair_strategies.insert(name, strategy);
    }
}

impl Handler<RecordRepairResult> for RepairExecutor {
    type Result = ();

    fn handle(&mut self, msg: RecordRepairResult, _ctx: &mut Self::Context) -> Self::Result {
        self.report_recipient.do_send(msg);
    }
}

// 处理来自 Supervisor 的 RepairRequest：将请求入队，记录并返回
impl Handler<RepairRequest> for RepairExecutor {
    type Result = ();

    fn handle(&mut self, msg: RepairRequest, ctx: &mut Self::Context) -> Self::Result {
        // 将请求加入待处理队列
        let req = msg.clone();
        let addr = ctx.address();
        // 根据 name 查找策略
        if let Some(strategy) = self.repair_strategies.get(&req.strategy).cloned() {
            // 启动后台任务执行修复逻辑，完成后把结果回送给自己

            actix::spawn(async move {
                info!("开始修复 {}...", strategy.name());
                let res = strategy.repair(req.clone()).await;
                let result = match res {
                    Ok(_) => RepairResult {
                        request_id: req.id,
                        strategy: req.strategy.clone(),
                        status: RepairStatus::SUCCEEDED,
                        error: None,
                    },
                    Err(e) => RepairResult {
                        request_id: req.id,
                        strategy: req.strategy.clone(),
                        status: RepairStatus::FAILED,
                        error: Some(e),
                    },
                };
                // 忽略发送错误
                let _ = addr.do_send(RecordRepairResult(result));
            });
        } else {
            // 未找到策略：记录为 SKIPPED 并从 pending 中移除
            error!("No repair strategy registered for {}", req.strategy);
            let result = RepairResult {
                request_id: req.id,
                strategy: req.strategy.clone(),
                status: RepairStatus::SKIPPED,
                error: Some("no strategy".to_string()),
            };
            let _ = addr.do_send(RecordRepairResult(result));
        }
    }
}

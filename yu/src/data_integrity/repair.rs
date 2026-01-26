// RepairExecutor 负责接收校验结果并调度修复请求（简化实现）
use crate::data_integrity::checker::ValidationResultMsg as CheckerValidationResultMsg;
use crate::data_integrity::{RepairRequest, RepairResult, RepairStatus};
use actix::prelude::*;
use std::collections::VecDeque;
use yue::tools::get_snow_flake_id_u64;

pub struct RepairExecutor {
    // 简单队列，持有待处理的请求（此处仅演示转换与存储）
    pub pending: VecDeque<RepairRequest>,
    pub results: Vec<RepairResult>,
}

impl Default for RepairExecutor {
    fn default() -> Self {
        RepairExecutor {
            pending: VecDeque::new(),
            results: Vec::new(),
        }
    }
}

impl Actor for RepairExecutor {
    type Context = Context<Self>;
}

/// 接收 Checker 的 ValidationResultMsg 并转换为 RepairRequest
impl Handler<CheckerValidationResultMsg> for RepairExecutor {
    type Result = ();

    fn handle(&mut self, msg: CheckerValidationResultMsg, _ctx: &mut Context<Self>) -> Self::Result {
        let vr = msg.0;
        // 如果没有 gaps，跳过（将状态记录为 SKIPPED）
        let id = get_snow_flake_id_u64().to_string();
        if vr.gaps.is_empty() {
            let r = RepairResult {
                request_id: id.clone(),
                strategy: vr.strategy.clone(),
                status: RepairStatus::SKIPPED,
                error: None,
            };
            self.results.push(r);
            return;
        }

        let req = RepairRequest {
            id: id.clone(),
            strategy: vr.strategy.clone(),
            gaps: vr.gaps.clone(),
        };
        self.pending.push_back(req);

        // 模拟修复成功并记录结果（同步示例）
        let r = RepairResult {
            request_id: id.clone(),
            strategy: vr.strategy.clone(),
            status: RepairStatus::SUCCEEDED,
            error: None,
        };
        self.results.push(r);
    }
}

/// 查询当前已完成的修复结果
pub struct GetRepairResults;
impl Message for GetRepairResults {
    type Result = Vec<RepairResult>;
}

impl Handler<GetRepairResults> for RepairExecutor {
    type Result = MessageResult<GetRepairResults>;

    fn handle(&mut self, _msg: GetRepairResults, _ctx: &mut Context<Self>) -> Self::Result {
        MessageResult(self.results.clone())
    }
}

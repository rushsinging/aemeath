//! interaction — 处理执行中断。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - `AwaitingUser`（ask_user）：暂停 Run 等待用户输入
//! - `AwaitingToolApproval`（权限门）：暂停 Run 等待审批
//! - pause/resume
//! - 触发 Run 状态机迁移到 `AwaitingUser` / `AwaitingToolApproval`
//!
//! 消费：`InteractionPort`（UI 交互）、`PolicyPort`（权限判断）
//!
//! 实现由 #1245 负责；生产适配接线由 #1246/#1248 承接。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use sdk::{
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionReplyError,
    InteractionRequest, InteractionRequestBody, InteractionRequestId, RunId,
};
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionCompletion {
    Replied(InteractionReply),
    Cancelled(InteractionCancelReason),
}

struct PendingWaiter {
    request: InteractionRequest,
    completion: oneshot::Sender<InteractionCompletion>,
}

#[derive(Default)]
struct BridgeState {
    pending: HashMap<InteractionRequestId, PendingWaiter>,
    completed: HashSet<InteractionRequestId>,
}

#[derive(Default)]
pub struct InteractionBridge {
    state: Mutex<BridgeState>,
}

impl InteractionBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionCompletion>, InteractionCommandOutcome> {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        if state.pending.contains_key(&request.id) || state.completed.contains(&request.id) {
            return Err(InteractionCommandOutcome::AlreadyCompleted);
        }
        let (completion, receiver) = oneshot::channel();
        state.pending.insert(
            request.id.clone(),
            PendingWaiter {
                request,
                completion,
            },
        );
        Ok(receiver)
    }

    pub fn contains(&self, request_id: &InteractionRequestId) -> bool {
        self.state
            .lock()
            .expect("interaction bridge poisoned")
            .pending
            .contains_key(request_id)
    }

    pub fn reply(
        &self,
        request_id: &InteractionRequestId,
        reply: InteractionReply,
    ) -> InteractionCommandOutcome {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        let Some(waiter) = state.pending.get(request_id) else {
            return completed_or_not_found(&state, request_id);
        };
        if let Err(error) = validate_reply(&waiter.request.body, &reply) {
            return InteractionCommandOutcome::InvalidReply(error);
        }
        let waiter = state.pending.remove(request_id).expect("checked above");
        state.completed.insert(request_id.clone());
        if waiter
            .completion
            .send(InteractionCompletion::Replied(reply))
            .is_err()
        {
            return InteractionCommandOutcome::RunCancelling;
        }
        InteractionCommandOutcome::Accepted
    }

    pub fn cancel(
        &self,
        request_id: &InteractionRequestId,
        reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        let Some(waiter) = state.pending.remove(request_id) else {
            return completed_or_not_found(&state, request_id);
        };
        state.completed.insert(request_id.clone());
        if waiter
            .completion
            .send(InteractionCompletion::Cancelled(reason))
            .is_err()
        {
            return InteractionCommandOutcome::RunCancelling;
        }
        InteractionCommandOutcome::Accepted
    }

    pub fn drain_run(&self, run_id: &RunId, reason: InteractionCancelReason) -> usize {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        let ids = state
            .pending
            .iter()
            .filter(|(_, waiter)| &waiter.request.run_id == run_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            if let Some(waiter) = state.pending.remove(id) {
                state.completed.insert(id.clone());
                let _ = waiter
                    .completion
                    .send(InteractionCompletion::Cancelled(reason.clone()));
            }
        }
        ids.len()
    }
}

fn completed_or_not_found(
    state: &BridgeState,
    request_id: &InteractionRequestId,
) -> InteractionCommandOutcome {
    if state.completed.contains(request_id) {
        InteractionCommandOutcome::AlreadyCompleted
    } else {
        InteractionCommandOutcome::NotFound
    }
}

fn validate_reply(
    body: &InteractionRequestBody,
    reply: &InteractionReply,
) -> Result<(), InteractionReplyError> {
    match (body, reply) {
        (
            InteractionRequestBody::UserQuestions(questions),
            InteractionReply::UserQuestions(answers),
        ) => {
            if questions.len() != answers.len() {
                Err(InteractionReplyError::AnswerCountMismatch)
            } else {
                Ok(())
            }
        }
        (InteractionRequestBody::ToolApproval(_), InteractionReply::ToolApproval(_))
        | (InteractionRequestBody::PlanApproval(_), InteractionReply::PlanApproval(_))
        | (InteractionRequestBody::HardPause(_), InteractionReply::HardPauseContinue) => Ok(()),
        _ => Err(InteractionReplyError::VariantMismatch),
    }
}

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;

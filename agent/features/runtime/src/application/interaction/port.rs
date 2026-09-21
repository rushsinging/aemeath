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
//! #1248 Task 4: 收敛为 Runtime-owned object-safe `InteractionPort` trait。
//! `InteractionBridge` 实现 client 端口；`UnavailableInteractionPort` 立即
//! 返回 typed `InteractionPortError::Unavailable`（非悬挂）。

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use sdk::{
    InteractionCancelReason, InteractionCommandOutcome, InteractionReply, InteractionReplyError,
    InteractionRequest, InteractionRequestBody, InteractionRequestId, RunId,
};
use tokio::sync::oneshot;

use crate::domain::agent_run::InteractionContinuation;

// ── Published types ──

/// Complete request metadata stored alongside a oneshot receiver.
/// #1248: Engine and port use this to track pending work and dispatch
/// resolutions — no request identity is lost across the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRequestMetadata {
    pub request_id: InteractionRequestId,
    pub run_id: RunId,
    pub body: InteractionRequestBody,
    pub continuation: InteractionContinuation,
}

impl InteractionRequestMetadata {
    pub fn new(
        request_id: InteractionRequestId,
        run_id: RunId,
        body: InteractionRequestBody,
        continuation: InteractionContinuation,
    ) -> Self {
        Self {
            request_id,
            run_id,
            body,
            continuation,
        }
    }
}

/// Resolution of a single pending interaction.
/// #1248: Replaces bare `InteractionCompletion` for poll return.
/// Each variant carries enough metadata for the engine to dispatch
/// the outcome — no need to look up state from a separate source.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionResolution {
    /// Resolved: completed normally with a reply.
    Resolved {
        metadata: InteractionRequestMetadata,
        completion: InteractionCompletion,
    },
    /// Closed: the interaction channel was dropped before resolution.
    Closed {
        metadata: InteractionRequestMetadata,
    },
}

impl InteractionResolution {
    pub fn metadata(&self) -> &InteractionRequestMetadata {
        match self {
            Self::Resolved { metadata, .. } | Self::Closed { metadata } => metadata,
        }
    }
}

/// Completion of a pending interaction (existing wire format kept for
/// backward compatibility with the oneshot channel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionCompletion {
    Replied(InteractionReply),
    Cancelled(InteractionCancelReason),
}

/// Port-level errors returned by [`InteractionPort::register`].
///
/// Distinguished from [`InteractionCommandOutcome`] (which is a
/// command-level result for `reply` / `cancel`).  This separation
/// ensures unavailable ports never hang and register failures are
/// typed rather than folded into `NotFound`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionPortError {
    /// Interaction is entirely unavailable for this run (e.g. sub-agent
    /// with `Unavailable` binding mode).  Any `register` call
    /// immediately fails without hanging.
    Unavailable,
    /// The request belongs to a different Run than this scoped adapter.
    WrongRun,
    /// The requested `InteractionRequestId` has already been registered
    /// or completed.
    AlreadyRegistered,
}

// ── Object-safe port trait ──

/// Runtime-owned port for interaction lifecycle management.
///
/// #1248 Task 4: Replaces the concrete `Arc<InteractionBridge>` with a
/// trait so that Client, ParentMediated, and Unavailable binding modes
/// share a unified contract — register, reply, cancel, drain, contains.
pub trait InteractionPort: Send + Sync {
    /// Register a pending interaction request.  Returns a oneshot receiver
    /// that will resolve when the interaction completes or is cancelled.
    ///
    /// The receiver MUST be resolved exactly once (via `reply`, `cancel`,
    /// or `drain_run`).  Dropping the receiver without resolution is
    /// detected by the port and surfaced as `RunCancelling`.
    fn register(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionCompletion>, InteractionPortError>;

    /// Check whether a request ID is currently pending.
    fn contains(&self, request_id: &InteractionRequestId) -> bool;

    /// Reply to a pending request with a body-validated reply.
    fn reply(
        &self,
        request_id: &InteractionRequestId,
        reply: InteractionReply,
    ) -> InteractionCommandOutcome;

    /// Cancel a pending request with a typed reason.
    fn cancel(
        &self,
        request_id: &InteractionRequestId,
        reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome;

    /// Drain all pending requests for a given run, cancelling them with
    /// the given reason.  Returns the count of drained requests.
    fn drain_run(&self, run_id: &RunId, reason: InteractionCancelReason) -> usize;
}

// ── Shared reply validation ──

/// Validate that a reply body matches the expected request body.
///
/// #1248 Task 4: Single shared validation function — used by both
/// [`InteractionBridge`] and [`InteractionCoordinator`].  No duplicate
/// match arms.
///
/// Exhaustive match covers all four body variants:
/// - `UserQuestions` → `UserQuestions` with matching answer count
/// - `ToolApproval` → `ToolApproval`
/// - `PlanApproval` → `PlanApproval`
/// - `HardPause` → `HardPauseContinue`
pub fn validate_reply(
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

// ── Internal bridge state ──

struct PendingWaiter {
    request: InteractionRequest,
    completion: oneshot::Sender<InteractionCompletion>,
}

#[derive(Default)]
struct BridgeState {
    pending: HashMap<InteractionRequestId, PendingWaiter>,
    completed: HashSet<InteractionRequestId>,
}

// ── InteractionBridge (Client port) ──

/// Client-side interaction port.  Holds a `Mutex<BridgeState>` and
/// processes `register`/`reply`/`cancel`/`drain_run`/`contains` in
/// critical sections.
///
/// #1248 Task 4: Implements [`InteractionPort`] for the Client binding mode.
/// `InteractionCapability` / `disabled()` have been removed — use
/// [`UnavailableInteractionPort`] for unavailable interaction.
pub struct InteractionBridge {
    state: Mutex<BridgeState>,
}

impl Default for InteractionBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractionBridge {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(BridgeState::default()),
        }
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

impl InteractionPort for InteractionBridge {
    fn register(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionCompletion>, InteractionPortError> {
        let request_id = request.id.clone();
        let run_id = request.run_id.clone();
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        if state.pending.contains_key(&request.id) || state.completed.contains(&request.id) {
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] register rejected request_id={} run_id={} pending_count={} completed_count={}",
                request_id,
                run_id,
                state.pending.len(),
                state.completed.len(),
            );
            return Err(InteractionPortError::AlreadyRegistered);
        }
        let (completion, receiver) = oneshot::channel();
        state.pending.insert(
            request.id.clone(),
            PendingWaiter {
                request,
                completion,
            },
        );
        log::debug!(
            target: crate::LOG_TARGET,
            "[interaction_bridge] registered request_id={} run_id={} pending_count={}",
            request_id,
            run_id,
            state.pending.len(),
        );
        Ok(receiver)
    }

    fn contains(&self, request_id: &InteractionRequestId) -> bool {
        self.state
            .lock()
            .expect("interaction bridge poisoned")
            .pending
            .contains_key(request_id)
    }

    fn reply(
        &self,
        request_id: &InteractionRequestId,
        reply: InteractionReply,
    ) -> InteractionCommandOutcome {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        let Some(waiter) = state.pending.get(request_id) else {
            let outcome = completed_or_not_found(&state, request_id);
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] reply missing request_id={} outcome={outcome:?} pending_count={} completed_count={}",
                request_id,
                state.pending.len(),
                state.completed.len(),
            );
            return outcome;
        };
        let run_id = waiter.request.run_id.clone();
        if let Err(error) = validate_reply(&waiter.request.body, &reply) {
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] reply invalid request_id={} run_id={} error={error:?}",
                request_id,
                run_id,
            );
            return InteractionCommandOutcome::InvalidReply(error);
        }
        let waiter = state.pending.remove(request_id).expect("checked above");
        state.completed.insert(request_id.clone());
        log::debug!(
            target: crate::LOG_TARGET,
            "[interaction_bridge] reply delivering request_id={} run_id={} receiver_closed={} pending_count={}",
            request_id,
            run_id,
            waiter.completion.is_closed(),
            state.pending.len(),
        );
        if waiter
            .completion
            .send(InteractionCompletion::Replied(reply))
            .is_err()
        {
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] reply delivery failed request_id={} run_id={} reason=receiver_closed",
                request_id,
                run_id,
            );
            return InteractionCommandOutcome::RunCancelling;
        }
        log::debug!(
            target: crate::LOG_TARGET,
            "[interaction_bridge] reply delivered request_id={} run_id={} outcome=Accepted",
            request_id,
            run_id,
        );
        InteractionCommandOutcome::Accepted
    }

    fn cancel(
        &self,
        request_id: &InteractionRequestId,
        reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome {
        let mut state = self.state.lock().expect("interaction bridge poisoned");
        let Some(waiter) = state.pending.remove(request_id) else {
            let outcome = completed_or_not_found(&state, request_id);
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] cancel missing request_id={} outcome={outcome:?} pending_count={} completed_count={}",
                request_id,
                state.pending.len(),
                state.completed.len(),
            );
            return outcome;
        };
        let run_id = waiter.request.run_id.clone();
        state.completed.insert(request_id.clone());
        log::debug!(
            target: crate::LOG_TARGET,
            "[interaction_bridge] cancel delivering request_id={} run_id={} receiver_closed={} reason={reason:?} pending_count={}",
            request_id,
            run_id,
            waiter.completion.is_closed(),
            state.pending.len(),
        );
        if waiter
            .completion
            .send(InteractionCompletion::Cancelled(reason))
            .is_err()
        {
            log::debug!(
                target: crate::LOG_TARGET,
                "[interaction_bridge] cancel delivery failed request_id={} run_id={} reason=receiver_closed",
                request_id,
                run_id,
            );
            return InteractionCommandOutcome::RunCancelling;
        }
        log::debug!(
            target: crate::LOG_TARGET,
            "[interaction_bridge] cancel delivered request_id={} run_id={} outcome=Accepted",
            request_id,
            run_id,
        );
        InteractionCommandOutcome::Accepted
    }

    fn drain_run(&self, run_id: &RunId, reason: InteractionCancelReason) -> usize {
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

/// Child-scoped adapter that routes interaction commands through the parent
/// capability without sharing the parent's port identity.
pub struct ParentMediatedInteractionPort {
    sub_run_id: Mutex<Option<RunId>>,
    parent: std::sync::Arc<dyn InteractionPort>,
    owned: Mutex<HashSet<InteractionRequestId>>,
}

impl ParentMediatedInteractionPort {
    pub fn new(parent: std::sync::Arc<dyn InteractionPort>) -> Self {
        Self {
            sub_run_id: Mutex::new(None),
            parent,
            owned: Mutex::new(HashSet::new()),
        }
    }

    fn owns(&self, request_id: &InteractionRequestId) -> bool {
        self.owned
            .lock()
            .expect("parent-mediated interaction ownership poisoned")
            .contains(request_id)
    }
}

impl InteractionPort for ParentMediatedInteractionPort {
    fn register(
        &self,
        request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionCompletion>, InteractionPortError> {
        let mut sub_run_id = self
            .sub_run_id
            .lock()
            .expect("parent-mediated run identity poisoned");
        match sub_run_id.as_ref() {
            Some(expected) if expected != &request.run_id => {
                return Err(InteractionPortError::WrongRun);
            }
            None => *sub_run_id = Some(request.run_id.clone()),
            _ => {}
        }
        let request_id = request.id.clone();
        let receiver = self.parent.register(request)?;
        self.owned
            .lock()
            .expect("parent-mediated interaction ownership poisoned")
            .insert(request_id);
        Ok(receiver)
    }

    fn contains(&self, request_id: &InteractionRequestId) -> bool {
        self.owns(request_id) && self.parent.contains(request_id)
    }

    fn reply(
        &self,
        request_id: &InteractionRequestId,
        reply: InteractionReply,
    ) -> InteractionCommandOutcome {
        if !self.owns(request_id) {
            return InteractionCommandOutcome::NotFound;
        }
        let outcome = self.parent.reply(request_id, reply);
        if !matches!(outcome, InteractionCommandOutcome::InvalidReply(_)) {
            self.owned
                .lock()
                .expect("parent-mediated interaction ownership poisoned")
                .remove(request_id);
        }
        outcome
    }

    fn cancel(
        &self,
        request_id: &InteractionRequestId,
        reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome {
        if !self.owns(request_id) {
            return InteractionCommandOutcome::NotFound;
        }
        let outcome = self.parent.cancel(request_id, reason);
        self.owned
            .lock()
            .expect("parent-mediated interaction ownership poisoned")
            .remove(request_id);
        outcome
    }

    fn drain_run(&self, run_id: &RunId, reason: InteractionCancelReason) -> usize {
        if self
            .sub_run_id
            .lock()
            .expect("parent-mediated run identity poisoned")
            .as_ref()
            != Some(run_id)
        {
            return 0;
        }
        let drained = self.parent.drain_run(run_id, reason);
        self.owned
            .lock()
            .expect("parent-mediated interaction ownership poisoned")
            .clear();
        drained
    }
}

// ── UnavailableInteractionPort ──

/// A port that rejects all interaction immediately.
///
/// `register` always returns [`InteractionPortError::Unavailable`]
/// without any async work or locking — guarantees no hanging.
///
/// #1248 Task 4: Typed unavailable — use this instead of the removed
/// `InteractionBridge::disabled()`.
#[derive(Debug, Clone, Default)]
pub struct UnavailableInteractionPort;

impl InteractionPort for UnavailableInteractionPort {
    fn register(
        &self,
        _request: InteractionRequest,
    ) -> Result<oneshot::Receiver<InteractionCompletion>, InteractionPortError> {
        Err(InteractionPortError::Unavailable)
    }

    fn contains(&self, _request_id: &InteractionRequestId) -> bool {
        false
    }

    fn reply(
        &self,
        _request_id: &InteractionRequestId,
        _reply: InteractionReply,
    ) -> InteractionCommandOutcome {
        InteractionCommandOutcome::NotFound
    }

    fn cancel(
        &self,
        _request_id: &InteractionRequestId,
        _reason: InteractionCancelReason,
    ) -> InteractionCommandOutcome {
        InteractionCommandOutcome::NotFound
    }

    fn drain_run(&self, _run_id: &RunId, _reason: InteractionCancelReason) -> usize {
        0
    }
}

#[cfg(test)]
#[path = "port_tests.rs"]
mod tests;

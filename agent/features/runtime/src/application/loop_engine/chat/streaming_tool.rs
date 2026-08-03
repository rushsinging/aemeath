//! #1494 边流边执行：流中 `ToolCallCompleted`（参数完整）即触发工具执行。
//!
//! 执行结果暂存缓冲，流正常结束后由 engine 统一汇总（materialize + 写入消息历史）；
//! 流失败（retry / compact）时缓冲在下次 invoke 开头清空——step 作废，重试请求
//! **不带**已执行工具结果（用户确认语义：异常时 step 不可用）。
//!
//! 设计约束（spec 3.4.5 / run 状态机）：
//! - 旁路执行**不**推进 run 状态机、**不**写消息历史——只发 TUI 事件（ToolCallStatus /
//!   ToolResult）并暂存 `ToolRoundResult`，由 engine 的 Tools 阶段统一登记与汇总。
//! - 并发受 `max_tool_concurrency` 限制（semaphore），与普通工具轮次一致。

use std::path::PathBuf;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::chat::tools::execute_tool_round;
use crate::application::loop_engine::chat::RuntimeTurnContext;
use crate::application::loop_engine::ToolGuardDecision;
use crate::application::run::context::RuntimeContext;
use crate::application::tool::agent::{Agent, ToolCall};

/// 一次旁路工具轮次的结果（与普通轮次 `execute_tool_round` 返回一致）。
pub(crate) type StreamingToolRoundResult =
    crate::application::loop_engine::chat::tools::ToolRoundResult;

/// 流事件投影器（reducer）依赖的提交端口：流中 `ToolCallCompleted` → 立即执行。
pub(crate) trait StreamingToolSubmitPort: Send + Sync {
    fn submit(&self, call: ToolCall);
}

/// 旁路执行句柄：流中 `submit` 立即执行（并发受 semaphore 限制），
/// 结果暂存缓冲；`take_results` 等待全部执行完成并取走。
pub(crate) struct StreamingToolExecutor {
    inner: Arc<StreamingToolInner>,
}

struct StreamingToolInner {
    runtime_context: Arc<RuntimeContext>,
    agent: Agent,
    turn_context: RuntimeTurnContext,
    run_id: sdk::RunId,
    language: String,
    workspace_root: PathBuf,
    semaphore: Arc<tokio::sync::Semaphore>,
    state: std::sync::Mutex<StreamingToolState>,
}

#[derive(Clone)]
struct StreamingInvocation {
    step_id: sdk::RunStepId,
    generation: u64,
    cancel: CancellationToken,
}

impl StreamingInvocation {
    fn new(step_id: sdk::RunStepId, generation: u64, cancel: CancellationToken) -> Self {
        Self {
            step_id,
            generation,
            cancel,
        }
    }
}

#[derive(Default)]
struct StreamingToolState {
    invocation: Option<StreamingInvocation>,
    next_generation: u64,
    pending: Vec<tokio::task::JoinHandle<()>>,
    results: Vec<StreamingToolRoundResult>,
}

impl StreamingToolState {
    fn begin_invocation(
        &mut self,
        step_id: &sdk::RunStepId,
        step_cancel: CancellationToken,
    ) -> StreamingInvocation {
        self.next_generation = self.next_generation.wrapping_add(1);
        let invocation = StreamingInvocation::new(
            step_id.clone(),
            self.next_generation,
            step_cancel.child_token(),
        );
        self.invocation = Some(invocation.clone());
        invocation
    }

    fn detach_invocation(&mut self) -> Vec<tokio::task::JoinHandle<()>> {
        if let Some(invocation) = self.invocation.take() {
            invocation.cancel.cancel();
        }
        std::mem::take(&mut self.pending)
    }

    fn accepts(&self, invocation: &StreamingInvocation) -> bool {
        self.invocation.as_ref().is_some_and(|active| {
            active.step_id == invocation.step_id && active.generation == invocation.generation
        })
    }

    fn accept_result(
        &mut self,
        invocation: &StreamingInvocation,
        round: StreamingToolRoundResult,
    ) -> bool {
        if !self.accepts(invocation) {
            return false;
        }
        self.results.push(round);
        true
    }
}

#[cfg(test)]
#[path = "streaming_tool_tests.rs"]
mod tests;

async fn await_pending_tasks(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

impl StreamingToolExecutor {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        runtime_context: Arc<RuntimeContext>,
        agent: Agent,
        turn_context: RuntimeTurnContext,
        run_id: sdk::RunId,
        language: String,
        workspace_root: PathBuf,
        max_tool_concurrency: usize,
    ) -> Self {
        Self {
            inner: Arc::new(StreamingToolInner {
                runtime_context,
                agent,
                turn_context,
                run_id,
                language,
                workspace_root,
                semaphore: Arc::new(tokio::sync::Semaphore::new(max_tool_concurrency.max(1))),
                state: std::sync::Mutex::new(StreamingToolState::default()),
            }),
        }
    }

    /// 每次 invoke 开始时绑定当前 Step scope，并先收敛上一次 invocation 的任务。
    pub(crate) async fn reset_for_invocation(
        &self,
        step_id: &sdk::RunStepId,
        step_cancel: CancellationToken,
    ) {
        let (handles, dropped_results, dropped_pending) = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let pending_handles = state.detach_invocation();
            let dropped_pending = pending_handles.len();
            (pending_handles, state.results.len(), dropped_pending)
        };
        await_pending_tasks(handles).await;
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let invocation = state.begin_invocation(step_id, step_cancel);
        state.results.clear();
        log::debug!(
            target: crate::LOG_TARGET,
            "[streaming_tool] reset_for_invocation step_id={} generation={} dropped_results={} pending={}",
            invocation.step_id,
            invocation.generation,
            dropped_results,
            dropped_pending
        );
    }
    /// 流中 `ToolCallCompleted` → 立即执行（spawn，semaphore 限并发）。
    fn submit(&self, call: ToolCall) {
        let inner = self.inner.clone();
        let (invocation, step_id) = {
            let state = inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let Some(invocation) = state.invocation.clone() else {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "streaming tool submit skipped: invocation not attached (call={})",
                    call.name
                );
                return;
            };
            (invocation.clone(), invocation.step_id.clone())
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "[streaming_tool] submit tool={} id={} index={}",
            call.name,
            call.id,
            call.index
        );
        let spawn_inner = inner.clone();
        let handle = tokio::spawn(async move {
            let _permit = match spawn_inner.semaphore.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return,
            };
            if invocation.cancel.is_cancelled() {
                return;
            }
            let guarded = vec![(call.clone(), ToolGuardDecision::Allow)];
            let round = execute_tool_round(
                &spawn_inner.turn_context,
                std::slice::from_ref(&call),
                &spawn_inner.agent.catalog,
                spawn_inner.runtime_context.policy_ref().as_ref(),
                &spawn_inner.run_id,
                &step_id,
                &spawn_inner.agent,
                &spawn_inner.runtime_context.event_sink(),
                spawn_inner.runtime_context.hooks_ref(),
                spawn_inner.runtime_context.activities().as_ref(),
                &invocation.cancel,
                &spawn_inner.language,
                &spawn_inner.workspace_root,
                &guarded,
            )
            .await;
            log::debug!(
                target: crate::LOG_TARGET,
                "[streaming_tool] round completed tool={} results={} suspensions={} approvals={}",
                call.name,
                round.results.len(),
                round.suspensions.len(),
                round.approvals.len()
            );
            let accepted = spawn_inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .accept_result(&invocation, round);
            if !accepted {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[streaming_tool] discarded stale round tool={} step_id={} generation={}",
                    call.name,
                    invocation.step_id,
                    invocation.generation
                );
            }
        });
        inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .pending
            .push(handle);
    }

    /// 等待全部旁路执行完成并取走结果（engine Tools 阶段调用）。
    pub(crate) async fn take_results(&self) -> Vec<StreamingToolRoundResult> {
        let handles = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            std::mem::take(&mut state.pending)
        };
        await_pending_tasks(handles).await;
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let results = std::mem::take(&mut state.results);
        log::debug!(
            target: crate::LOG_TARGET,
            "[streaming_tool] take_results rounds={}",
            results.len()
        );
        results
    }
}

impl StreamingToolSubmitPort for StreamingToolExecutor {
    fn submit(&self, call: ToolCall) {
        StreamingToolExecutor::submit(self, call);
    }
}

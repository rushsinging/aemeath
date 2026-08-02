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
    cancel: CancellationToken,
    semaphore: Arc<tokio::sync::Semaphore>,
    state: std::sync::Mutex<StreamingToolState>,
}

#[derive(Default)]
struct StreamingToolState {
    step_id: Option<sdk::RunStepId>,
    pending: Vec<tokio::task::JoinHandle<()>>,
    results: Vec<StreamingToolRoundResult>,
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
        cancel: CancellationToken,
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
                cancel,
                semaphore: Arc::new(tokio::sync::Semaphore::new(max_tool_concurrency.max(1))),
                state: std::sync::Mutex::new(StreamingToolState::default()),
            }),
        }
    }

    /// 每次 invoke 开始时调用：绑定 step 并清空上次残留（retry / compact 丢弃）。
    pub(crate) fn reset_for_invocation(&self, step_id: &sdk::RunStepId) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let dropped_results = state.results.len();
        let dropped_pending = state.pending.len();
        state.step_id = Some(step_id.clone());
        state.pending.clear();
        state.results.clear();
        log::debug!(
            target: crate::LOG_TARGET,
            "[streaming_tool] reset_for_invocation dropped_results={} pending={}",
            dropped_results,
            dropped_pending
        );
    }

    /// 流中 `ToolCallCompleted` → 立即执行（spawn，semaphore 限并发）。
    fn submit(&self, call: ToolCall) {
        let inner = self.inner.clone();
        let step_id = {
            let state = inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.step_id.clone()
        };
        let Some(step_id) = step_id else {
            log::warn!(
                target: crate::LOG_TARGET,
                "streaming tool submit skipped: step_id not attached (call={})",
                call.name
            );
            return;
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
            if spawn_inner.cancel.is_cancelled() {
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
                &spawn_inner.cancel,
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
            spawn_inner
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .results
                .push(round);
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
        for handle in handles {
            let _ = handle.await;
        }
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

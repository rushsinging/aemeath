//! context_coordination — 构建本轮 Context Window。
//!
//! 对应设计：`docs/design/02-modules/runtime/02-module-boundaries.md` §2。
//!
//! 职责：
//! - 取历史消息
//! - compact 家族（L2 snip / L3 microcompact / L4 collapse / L5 auto-compact）
//! - memory 注入
//! - prompt/guidance 装配
//! - token budget 计算
//!
//! 消费：`ContextPort`（Context Management BC）、`MemoryPort`
//!
//! 注：Session 对话历史属 Context Management，本模块只是 Runtime 侧调用协调。
//! Memory 边界：检索归 Memory（`MemoryPort.retrieve`），注入进 Context Window 归
//! Context Management——记忆本体是独立 BC，不是 Context 的一部分。
//!
//! Runtime 只在本模块协调 Context-owned OHS；冻结 request 的字段映射留在调用方，
//! 不扩展第五个 ContextPort 方法。

use crate::ports::{
    AcceptedInputAppendData, AcceptedInputError, CompactOutcome, CompactRequestData,
    CompactTrigger, ContextAppendData, ContextAppendError, ContextPortError, ContextRequestData,
    ContextWindowData, FinalizeCause, RunStepReceiptData, SessionAppendReceiptData, SessionId,
    SessionRevision,
};
use context::{ToolReceiptMutationData, ToolReceiptMutationError, ToolReceiptMutationReceiptData};
use sdk::RunStepId;
use share::message::Message;

/// Apply the Runtime-owned state transition for an automatic compact outcome.
///
/// A committed compact invalidates both the Provider usage baseline and the
/// materialized ContextWindowData. A typed skip is a non-fatal no-op: Context did
/// not mutate its backing, so Runtime keeps both values and proceeds with the
/// current model invocation.
/// #1385 TaskData 12: `last_total_tokens` replaced with `RunUsageTracker` for
/// RuntimeContext I/O seam.
pub(crate) fn apply_automatic_compact_outcome<T>(
    outcome: &CompactOutcome,
    usage: &crate::application::run::context::RunUsageTracker,
    context_window: &mut Option<T>,
) {
    if matches!(outcome, CompactOutcome::Committed(_)) {
        usage.reset();
        *context_window = None;
    }
}

/// Runtime 对 Context-owned 四方法端口的单一协调 façade。
#[derive(Clone)]
pub(crate) struct ContextCoordinator {
    ports: context::ContextPorts,
    /// compact 模型解析器（#2142）：`None` 时 compact 请求 `llm: None` 走本地压缩。
    compact: Option<std::sync::Arc<crate::application::client::CompactModelResolver>>,
}

impl ContextCoordinator {
    pub(crate) fn new(ports: context::ContextPorts) -> Self {
        Self {
            ports,
            compact: None,
        }
    }

    /// 绑定 compact 模型解析器（#2142）：manual 与自动压缩共用同一 resolver
    /// 单一入口（模型/窗口/预算按轮解析，失败回退本地并保留 warn 日志）。
    pub(crate) fn with_compact_resolver(
        mut self,
        resolver: Option<std::sync::Arc<crate::application::client::CompactModelResolver>>,
    ) -> Self {
        self.compact = resolver;
        self
    }

    /// 本轮 compact 的 LLM 绑定（`None` = 本地压缩）。
    fn compact_llm(&self) -> Option<context::CompactLlmBinding> {
        self.compact
            .as_ref()
            .and_then(|resolver| resolver.to_llm_binding())
    }

    /// Reminder 事件触发转发（#252：后台进程完成 → OnEvent source 重建）。
    pub(crate) fn reminder_handle_event(
        &self,
        run_id: &sdk::RunId,
        event_source: &context::ReminderEventSource,
    ) {
        self.ports
            .reminders
            .reminder_handle_event(run_id, event_source);
    }

    pub(crate) async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        log::debug!(
            target: crate::LOG_TARGET,
            "context_request_forwarded request_id={} run_id={} step_id={}",
            request.request_id.as_str(),
            request.run_id,
            request.step_id.as_str(),
        );
        let window = self.ports.window.build_window(request).await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "context_window_received request_id={} revision={} messages={} system_blocks={} tool_schemas={}",
            request.request_id.as_str(),
            window.backing_revision.get(),
            window.messages.len(),
            window.system_blocks.len(),
            window.tool_schemas.len(),
        );
        Ok(window)
    }

    pub(crate) async fn compaction_decision(
        &self,
        request: &ContextRequestData,
    ) -> Result<context::CompactionDecisionData, ContextPortError> {
        self.ports.compact.needs_compaction(request).await
    }

    #[cfg(test)]
    pub(crate) async fn needs_compaction(
        &self,
        request: &ContextRequestData,
    ) -> Result<bool, ContextPortError> {
        Ok(self.compaction_decision(request).await?.needed)
    }

    /// 手动压缩入口：compact(trigger: Manual)（#1863 单一压缩操作）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn manual_compact(
        &self,
        request: &ContextRequestData,
        progress: std::sync::Arc<dyn crate::application::loop_engine::CompactProgressView>,
        task_snapshot: Option<task::CompactTaskSnapshotData>,
    ) -> Result<CompactOutcome, ContextPortError> {
        let progress: Option<std::sync::Arc<dyn context::compact::CompactProgressFn>> =
            Some(crate::application::loop_engine::compaction::compact_progress_adapter(progress));
        self.ports
            .compact
            .compact(&CompactRequestData {
                llm: self.compact_llm(),
                run_id: request.run_id.clone(),
                source_revision: SessionRevision::new(0),
                source: request.clone(),
                trigger: CompactTrigger::Manual,
                progress,
                task_snapshot,
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await
    }

    pub(crate) async fn compact(
        &self,
        request: &ContextRequestData,
        source_revision: SessionRevision,
        progress: std::sync::Arc<dyn crate::application::loop_engine::CompactProgressView>,
        task_snapshot: Option<task::CompactTaskSnapshotData>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<CompactOutcome, ContextPortError> {
        let progress: Option<std::sync::Arc<dyn context::compact::CompactProgressFn>> =
            Some(crate::application::loop_engine::compaction::compact_progress_adapter(progress));
        self.ports
            .compact
            .compact(&CompactRequestData {
                llm: self.compact_llm(),
                run_id: request.run_id.clone(),
                source_revision,
                source: request.clone(),
                trigger: CompactTrigger::Automatic,
                progress,
                task_snapshot,
                cancellation,
            })
            .await
    }

    pub(crate) async fn clear_session(
        &self,
        session_id: &SessionId,
    ) -> Result<(), ContextPortError> {
        self.ports.append.clear_session(session_id).await
    }

    pub(crate) async fn append_accepted_input(
        &self,
        request: &ContextRequestData,
        messages: Vec<Message>,
    ) -> Result<SessionAppendReceiptData, AcceptedInputError> {
        self.ports
            .append
            .append_accepted_input(&AcceptedInputAppendData {
                session_id: request.session_id.clone(),
                run_id: request.run_id.clone(),
                step_id: request.step_id.clone(),
                source_request_id: request.request_id.clone(),
                messages,
            })
            .await
    }

    pub(crate) async fn advance_tool_receipt(
        &self,
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        self.ports.append.advance_tool_receipt(mutation).await
    }

    /// 会话当前提交修订号：Step 收口前刷新 CAS 基准，避免把本 step 自身的
    /// accepted-input / 工具回执推进误判为并发冲突。`None` 表示实现对调用方
    /// 不暴露修订号（由调用方沿用窗口快照值）。
    pub(crate) async fn current_revision(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<SessionRevision>, ContextPortError> {
        self.ports.append.current_revision(session_id).await
    }

    pub(crate) async fn step_receipts(
        &self,
        request: &ContextRequestData,
    ) -> Result<Vec<RunStepReceiptData>, ToolReceiptMutationError> {
        self.ports
            .append
            .step_receipts(&request.session_id, &request.run_id, &request.step_id)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn append_finalized(
        &self,
        request: &ContextRequestData,
        step_id: RunStepId,
        expected_revision: SessionRevision,
        finalize_cause: FinalizeCause,
        duration_ms: Option<u64>,
        messages: Vec<Message>,
        receipts: Vec<RunStepReceiptData>,
        api_input_tokens: Option<u64>,
    ) -> Result<SessionAppendReceiptData, ContextAppendError> {
        self.ports
            .append
            .append_and_persist(&ContextAppendData {
                session_id: request.session_id.clone(),
                expected_revision,
                run_id: request.run_id.clone(),
                step_id,
                source_request_id: request.request_id.clone(),
                finalize_cause,
                duration_ms,
                messages,
                receipts,
                api_input_tokens,
            })
            .await
    }
}

#[cfg(test)]
#[path = "coordination_tests.rs"]
mod tests;

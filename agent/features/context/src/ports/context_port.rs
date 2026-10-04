//! ContextPort — Context Management 对 Runtime 发布的类型化 OHS。

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::reminder::{ReminderEventSource, ReminderSource};
pub use crate::domain::*;

/// Context Management 对 Agent Runtime 开放的唯一端口。
///
/// Runtime 每个 RunStep 开始时构建 window；需要时执行 compact；普通完成、
/// CancelRunStep 或 TerminateRun 经 StepFinalizer 收口后提交唯一 ContextAppendData。
#[async_trait]
pub trait ContextPort: Send + Sync {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError>;

    async fn needs_compaction(
        &self,
        request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError>;

    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError>;

    async fn manual_compact(
        &self,
        request: &ManualCompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError>;

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError>;

    async fn append_accepted_input(
        &self,
        _append: &AcceptedInputAppendData,
    ) -> Result<AcceptedInputReceiptData, AcceptedInputError> {
        Err(AcceptedInputError::Storage(
            "此 ContextPort 未实现已接受输入持久化".to_string(),
        ))
    }

    async fn advance_tool_receipt(
        &self,
        _mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        Err(ToolReceiptMutationError::Storage(
            "此 ContextPort 未实现 Tool receipt 持久化".to_string(),
        ))
    }

    async fn step_receipts(
        &self,
        _session_id: &SessionId,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
    ) -> Result<Vec<StepReceiptData>, ToolReceiptMutationError> {
        Err(ToolReceiptMutationError::Storage(
            "此 ContextPort 未实现 Step receipt 查询".to_string(),
        ))
    }

    async fn compare_and_record_skill_load(
        &self,
        _mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        Err(tools::published::skill::SkillLoadStateError::Storage(
            "此 ContextPort 未实现 Skill 加载状态持久化".to_string(),
        ))
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError>;

    // ─── Reminder 统一管线控制面（07-reminder-pipeline.md）────────────
    //
    // Run 生命周期句柄与 typed 事件推送。默认 no-op：测试替身与不接入
    // reminder 管线的 ContextPort 实现保持兼容；生产实现由
    // ContextApplicationService 提供。注入决策在 build_window 内部
    // 按 policy 自动完成，Runtime 不感知 placement / dedup / 预算。

    /// Run 启动：为该 Run 创建 reminder 管线（同 RunId 重复创建替换旧管线）。
    fn create_reminder_pipeline(&self, _run_id: RunId, _sources: Vec<Arc<dyn ReminderSource>>) {}

    /// Run 结束：销毁该 Run 的 reminder 管线。
    fn drop_reminder_pipeline(&self, _run_id: &RunId) {}

    /// Run 启动事件：`OnRunStart` 类 source 入队。
    fn reminder_run_started(&self, _run_id: &RunId) {}

    /// 业务事件推送：`OnEvent(source)` 匹配的 source 入队。
    fn reminder_handle_event(&self, _run_id: &RunId, _event_source: &ReminderEventSource) {}

    /// task store 变更事件：`OnTaskMutation` 类 source 入队。
    fn reminder_task_mutated(&self, _run_id: &RunId) {}

    /// step 边界事件：`OnStepInterval(n)` 在步数为 n 的倍数时重建入队。
    fn reminder_step_advanced(&self, _run_id: &RunId, _step: u64) {}
}

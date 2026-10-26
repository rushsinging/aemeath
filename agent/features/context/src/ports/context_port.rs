//! Context 语义端口族（Window / Compact / Append / ReminderControl）——对 Runtime 的类型化 OHS。

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::reminder::{ReminderEventSource, ReminderSource};
pub use crate::domain::*;

/// Context Management 对 Agent Runtime 开放的唯一端口。
///
/// Runtime 每个 RunStep 开始时构建 window；需要时执行 compact；普通完成、
/// CancelRunStep 或 TerminateRun 经 StepFinalizer 收口后提交唯一 ContextAppendData。
#[async_trait]
/// Window 构建端口：组装本轮上下文（注入内容 + 预算分配）。
#[async_trait]
pub trait ContextWindowPort: Send + Sync {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError>;
}

/// 压缩端口：L1–L5 压缩决策与执行。
#[async_trait]
pub trait ContextCompactPort: Send + Sync {
    async fn needs_compaction(
        &self,
        request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError>;

    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError>;
}

/// 回写端口：历史 append、回执推进与查询。
#[async_trait]
pub trait ContextAppendPort: Send + Sync {
    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError>;

    async fn append_accepted_input(
        &self,
        _append: &AcceptedInputAppendData,
    ) -> Result<SessionAppendReceiptData, AcceptedInputError> {
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
    ) -> Result<Vec<RunStepReceiptData>, ToolReceiptMutationError> {
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
    ) -> Result<SessionAppendReceiptData, ContextAppendError>;
}

/// Reminder 管线控制面（07-reminder-pipeline.md）：Run 生命周期句柄与
/// typed 事件推送。注入决策在 build_window 内部按 policy 自动完成，
/// Runtime 不感知 placement / dedup / 预算。
pub trait ReminderControlPort: Send + Sync {
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

    /// 用户消息到达事件：`OnUserMessage` 类 source 重建入队。
    fn reminder_user_message_received(&self, _run_id: &RunId) {}
}

/// 四个语义端口的聚合视图（factory 产出形态；消费方按字段取所需 port）。
#[derive(Clone)]
pub struct ContextPorts {
    pub window: Arc<dyn ContextWindowPort>,
    pub compact: Arc<dyn ContextCompactPort>,
    pub append: Arc<dyn ContextAppendPort>,
    pub reminders: Arc<dyn ReminderControlPort>,
}

impl ContextPorts {
    /// 单一服务实例的四端口视图（同一 `Arc`，方法分派到同一实现）。
    pub fn from_service<T>(service: Arc<T>) -> Self
    where
        T: ContextWindowPort
            + ContextCompactPort
            + ContextAppendPort
            + ReminderControlPort
            + 'static,
    {
        Self {
            window: service.clone(),
            compact: service.clone(),
            append: service.clone(),
            reminders: service,
        }
    }
}

//! Context 四端口测试替身（#2146 实现权治理：ContextWindowPort /
//! ContextCompactPort / ContextAppendPort / ReminderControlPort 实现权归本
//! crate，跨 crate 测试经此获得替身；生产不编译）。
//!
//! 两件套：
//! - [`recording_context_over`]：装饰器——包装任意 inner `ContextPorts`，
//!   对持久化类调用（compact / append 家族 / reminder_handle_event）记录
//!   [`RecordedContextCall`] 后转发；用于验证「哪些持久化调用实际发生」。
//! - [`StubContextService`]：独立响应替身——默认成功固定响应，
//!   `compact_outcome` 可配置（take 一次）、`compact_calls` 记录压缩请求、
//!   `unavailable` 模式恒 Err（fakes 场景）。

use std::sync::{Arc, Mutex};

use crate::ports::context_port::{
    AcceptedInputAppendData, AcceptedInputError, CompactOutcome, CompactRequestData,
    CompactTrigger, CompactionDecisionData, ContextAppendData, ContextAppendError,
    ContextAppendPort, ContextCompactPort, ContextPortError, ContextPorts, ContextRequestData,
    ContextWindowPort, SessionAppendReceiptData, SessionRevision,
};
use crate::ports::context_port::{
    ReminderControlPort, RunStepReceiptData, ToolReceiptMutationError,
};
use async_trait::async_trait;
use sdk::RunId;

/// 装饰器记录的持久化调用（方法名 + 关键参数快照）。
#[derive(Debug, Clone, PartialEq)]
pub enum RecordedContextCall {
    Compact { trigger: &'static str },
    ClearSession,
    AppendAcceptedInput,
    AdvanceToolReceipt { next: String },
    StepReceipts { count: usize },
    CompareAndRecordSkillLoad,
    AppendAndPersist,
    ReminderHandleEvent,
}

impl RecordedContextCall {
    /// 方法名视图（与原 persistence_calls 断言口径一致）。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Compact { .. } => "compact",
            Self::ClearSession => "clear_session",
            Self::AppendAcceptedInput => "append_accepted_input",
            Self::AdvanceToolReceipt { .. } => "advance_tool_receipt",
            Self::StepReceipts { .. } => "step_receipts",
            Self::CompareAndRecordSkillLoad => "compare_and_record_skill_load",
            Self::AppendAndPersist => "append_and_persist",
            Self::ReminderHandleEvent => "reminder_handle_event",
        }
    }
}

struct RecordingContext {
    inner: ContextPorts,
    calls: Arc<Mutex<Vec<RecordedContextCall>>>,
}

/// 包装 inner：返回（新 ContextPorts，调用记录句柄）。
pub fn recording_context_over(
    inner: &ContextPorts,
) -> (ContextPorts, Arc<Mutex<Vec<RecordedContextCall>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ports = recording_context_shared(inner, &calls);
    (ports, calls)
}

/// 包装 inner 并把记录写入外接句柄（同一句柄可多次 wrap 共享一份记录）。
pub fn recording_context_shared(
    inner: &ContextPorts,
    calls: &Arc<Mutex<Vec<RecordedContextCall>>>,
) -> ContextPorts {
    let recorder = Arc::new(RecordingContext {
        inner: inner.clone(),
        calls: Arc::clone(calls),
    });
    ContextPorts {
        window: Arc::clone(&recorder.inner.window),
        compact: Arc::clone(&recorder) as Arc<dyn ContextCompactPort>,
        append: Arc::clone(&recorder) as Arc<dyn ContextAppendPort>,
        reminders: Arc::clone(&recorder) as Arc<dyn ReminderControlPort>,
    }
}

#[async_trait]
impl ContextCompactPort for RecordingContext {
    async fn needs_compaction(
        &self,
        request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        self.inner.compact.needs_compaction(request).await
    }
    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        let trigger = match request.trigger {
            CompactTrigger::Automatic => "automatic",
            CompactTrigger::Manual => "manual",
        };
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::Compact { trigger });
        self.inner.compact.compact(request).await
    }
}

#[async_trait]
impl ContextAppendPort for RecordingContext {
    async fn clear_session(
        &self,
        session_id: &share::ids::SessionId,
    ) -> Result<(), ContextPortError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::ClearSession);
        self.inner.append.clear_session(session_id).await
    }
    async fn current_revision(
        &self,
        session_id: &share::ids::SessionId,
    ) -> Result<Option<SessionRevision>, ContextPortError> {
        self.inner.append.current_revision(session_id).await
    }
    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<SessionAppendReceiptData, AcceptedInputError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::AppendAcceptedInput);
        self.inner.append.append_accepted_input(append).await
    }
    async fn advance_tool_receipt(
        &self,
        mutation: crate::ToolReceiptMutationData,
    ) -> Result<crate::ToolReceiptMutationReceiptData, crate::ToolReceiptMutationError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::AdvanceToolReceipt {
                next: format!("{:?}", mutation.next),
            });
        self.inner.append.advance_tool_receipt(mutation).await
    }
    async fn step_receipts(
        &self,
        session_id: &share::ids::SessionId,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
    ) -> Result<Vec<RunStepReceiptData>, ToolReceiptMutationError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::StepReceipts { count: 0 });
        self.inner
            .append
            .step_receipts(session_id, run_id, step_id)
            .await
    }
    async fn compare_and_record_skill_load(
        &self,
        mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::CompareAndRecordSkillLoad);
        self.inner
            .append
            .compare_and_record_skill_load(mutation)
            .await
    }
    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<SessionAppendReceiptData, ContextAppendError> {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::AppendAndPersist);
        self.inner.append.append_and_persist(append).await
    }
}

impl ReminderControlPort for RecordingContext {
    fn create_reminder_pipeline(
        &self,
        run_id: RunId,
        sources: Vec<Arc<dyn crate::ReminderSource>>,
    ) {
        self.inner
            .reminders
            .create_reminder_pipeline(run_id, sources)
    }
    fn drop_reminder_pipeline(&self, run_id: &RunId) {
        self.inner.reminders.drop_reminder_pipeline(run_id)
    }
    fn reminder_run_started(&self, run_id: &RunId) {
        self.inner.reminders.reminder_run_started(run_id)
    }
    fn reminder_handle_event(&self, run_id: &RunId, event_source: &crate::ReminderEventSource) {
        self.calls
            .lock()
            .unwrap()
            .push(RecordedContextCall::ReminderHandleEvent);
        self.inner
            .reminders
            .reminder_handle_event(run_id, event_source)
    }
    fn reminder_task_mutated(&self, run_id: &RunId) {
        self.inner.reminders.reminder_task_mutated(run_id)
    }
    fn reminder_step_advanced(&self, run_id: &RunId, step: u64) {
        self.inner.reminders.reminder_step_advanced(run_id, step)
    }
    fn reminder_user_message_received(&self, run_id: &RunId) {
        self.inner.reminders.reminder_user_message_received(run_id)
    }
}

// ─── 独立响应替身（成功固定响应 / 恒错 / compact 可配置） ───

/// 独立响应替身：默认成功固定响应；`set_compact_outcome` 配置下一次 compact
/// 结果（take 一次）；`unavailable(message)` 构造恒错形态；
/// `calls` 记录收到的方法调用名；`compact_requests` 记录压缩请求。
pub struct StubContextService {
    message: String,
    fail_all: bool,
    /// window/compact（读路径）恒 Err(SessionRepository)；append 成功。
    fail_read_only: bool,
    receipt_states: Mutex<Vec<String>>,
    compact_outcome: Mutex<Option<Result<CompactOutcome, ContextPortError>>>,
    calls: Mutex<Vec<&'static str>>,
    compact_requests: Mutex<Vec<CompactRequestData>>,
    accepted_inputs: Mutex<Vec<AcceptedInputAppendData>>,
    append_and_persist_inputs: Mutex<Vec<ContextAppendData>>,
}

impl StubContextService {
    /// 成功固定响应形态。
    pub fn new() -> Self {
        Self {
            message: String::new(),
            fail_all: false,
            fail_read_only: false,
            receipt_states: Mutex::new(Vec::new()),
            compact_outcome: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            compact_requests: Mutex::new(Vec::new()),
            accepted_inputs: Mutex::new(Vec::new()),
            append_and_persist_inputs: Mutex::new(Vec::new()),
        }
    }
    /// 读路径（window/needs/compact）恒 Err(SessionRepository(message))，
    /// append 家族成功；advance_tool_receipt 记录 receipt 状态快照。
    pub fn read_only_failing(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fail_all: false,
            fail_read_only: true,
            receipt_states: Mutex::new(Vec::new()),
            compact_outcome: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            compact_requests: Mutex::new(Vec::new()),
            accepted_inputs: Mutex::new(Vec::new()),
            append_and_persist_inputs: Mutex::new(Vec::new()),
        }
    }
    /// advance_tool_receipt 收到的状态快照序列（format next）。
    pub fn receipt_states(&self) -> Vec<String> {
        self.receipt_states.lock().unwrap().clone()
    }
    /// 恒错形态（所有方法返回 `Compact(message)`）。
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fail_all: true,
            fail_read_only: false,
            receipt_states: Mutex::new(Vec::new()),
            compact_outcome: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
            compact_requests: Mutex::new(Vec::new()),
            accepted_inputs: Mutex::new(Vec::new()),
            append_and_persist_inputs: Mutex::new(Vec::new()),
        }
    }
    /// 配置下一次 compact 结果（仅成功形态有效；take 一次）。
    pub fn set_compact_outcome(&self, outcome: Result<CompactOutcome, ContextPortError>) {
        *self.compact_outcome.lock().unwrap() = Some(outcome);
    }
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
    pub fn compact_requests(&self) -> Vec<CompactRequestData> {
        self.compact_requests.lock().unwrap().clone()
    }
    pub fn accepted_inputs(&self) -> Vec<AcceptedInputAppendData> {
        self.accepted_inputs.lock().unwrap().clone()
    }
    pub fn append_and_persist_inputs(&self) -> Vec<ContextAppendData> {
        self.append_and_persist_inputs.lock().unwrap().clone()
    }
    fn err(&self) -> ContextPortError {
        ContextPortError::Compact(self.message.clone())
    }
    fn record(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
}

impl Default for StubContextService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextWindowPort for StubContextService {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<crate::ports::context_port::ContextWindowData, ContextPortError> {
        self.record("build_window");
        if self.fail_all {
            return Err(self.err());
        }
        if self.fail_read_only {
            return Err(ContextPortError::SessionRepository(self.message.clone()));
        }
        Ok(crate::ports::context_port::ContextWindowData {
            backing_revision: SessionRevision::new(1),
            system_blocks: vec![crate::ports::context_port::SystemBlock {
                kind: "system_prompt".to_string(),
                content: "system".to_string(),
                cacheable: true,
                cache_break: true,
            }],
            messages: request.pending_messages.clone().into(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: crate::ports::context_port::TokenBudget {
                system_tokens: 2,
                tool_schema_tokens: 3,
                message_tokens: 5,
                total_tokens: 10,
            },
            compaction_decision: crate::ports::context_port::CompactionDecisionData {
                needed: false,
                urgency: crate::ports::context_port::Urgency::None,
                decision_token_count: 0,
                threshold: 1,
                context_size: 200_000,
                effective_window: 180_000,
                reason: crate::ports::context_port::DecisionReason::HeuristicFallback,
            },
        })
    }
}

#[async_trait]
impl ContextCompactPort for StubContextService {
    async fn needs_compaction(
        &self,
        _request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        self.record("needs_compaction");
        if self.fail_all {
            return Err(self.err());
        }
        if self.fail_read_only {
            return Err(ContextPortError::SessionRepository(self.message.clone()));
        }
        Ok(CompactionDecisionData {
            needed: true,
            urgency: crate::ports::context_port::Urgency::Must,
            decision_token_count: 100,
            threshold: 90,
            context_size: 200_000,
            effective_window: 180_000,
            reason: crate::ports::context_port::DecisionReason::ActualProviderUsage,
        })
    }
    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.record("compact");
        self.compact_requests.lock().unwrap().push(request.clone());
        if let Some(outcome) = self.compact_outcome.lock().unwrap().take() {
            return outcome;
        }
        if self.fail_all {
            return Err(self.err());
        }
        if self.fail_read_only {
            return Err(ContextPortError::SessionRepository(self.message.clone()));
        }
        Ok(CompactOutcome::Committed(
            crate::ports::context_port::CompactResult {
                summary: "summary".to_string(),
                recent_messages: request.source.pending_messages.clone(),
                source_revision: SessionRevision::new(1),
                quality: crate::ports::context_port::CompactSummaryQuality::LocalOnly,
            },
        ))
    }
}

#[async_trait]
impl ContextAppendPort for StubContextService {
    async fn clear_session(
        &self,
        _session_id: &share::ids::SessionId,
    ) -> Result<(), ContextPortError> {
        self.record("clear_session");
        if self.fail_all {
            Err(self.err())
        } else {
            Ok(())
        }
    }
    async fn current_revision(
        &self,
        _session_id: &share::ids::SessionId,
    ) -> Result<Option<SessionRevision>, ContextPortError> {
        self.record("current_revision");
        if self.fail_all {
            Err(self.err())
        } else {
            Ok(Some(SessionRevision::new(7)))
        }
    }
    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<SessionAppendReceiptData, AcceptedInputError> {
        self.record("append_accepted_input");
        self.accepted_inputs.lock().unwrap().push(append.clone());
        if self.fail_all {
            return Err(AcceptedInputError::Storage(self.message.clone()));
        }
        Ok(SessionAppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(2),
        })
    }
    async fn advance_tool_receipt(
        &self,
        mutation: crate::ToolReceiptMutationData,
    ) -> Result<crate::ToolReceiptMutationReceiptData, crate::ToolReceiptMutationError> {
        self.record("advance_tool_receipt");
        if self.fail_all {
            return Err(crate::ToolReceiptMutationError::Storage(
                self.message.clone(),
            ));
        }
        self.receipt_states
            .lock()
            .unwrap()
            .push(format!("{:?}", mutation.next));
        Ok(crate::ToolReceiptMutationReceiptData {
            receipt: crate::domain::tool_receipt::ToolCallReceiptData {
                identity: mutation.identity,
                input_preview: mutation.input_preview.unwrap_or_default(),
                state: mutation.next,
            },
            changed: true,
        })
    }
    async fn step_receipts(
        &self,
        _session_id: &share::ids::SessionId,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
    ) -> Result<Vec<RunStepReceiptData>, ToolReceiptMutationError> {
        self.record("step_receipts");
        if self.fail_all {
            return Err(ToolReceiptMutationError::Storage(self.message.clone()));
        }
        Ok(Vec::new())
    }
    async fn compare_and_record_skill_load(
        &self,
        _mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        self.record("compare_and_record_skill_load");
        Err(tools::published::skill::SkillLoadStateError::Storage(
            "stub 未实现 skill 加载状态".to_string(),
        ))
    }
    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<SessionAppendReceiptData, ContextAppendError> {
        self.record("append_and_persist");
        self.append_and_persist_inputs
            .lock()
            .unwrap()
            .push(append.clone());
        if self.fail_all {
            return Err(ContextAppendError::Storage(self.message.clone()));
        }
        Ok(SessionAppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(2),
        })
    }
}

impl ReminderControlPort for StubContextService {}

// #2146 密封解锁（测试替身）。
impl crate::sealed::Sealed for RecordingContext {}
impl crate::sealed::Sealed for StubContextService {}

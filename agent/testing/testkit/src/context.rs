//! context 测试替身（#2146 治理后统一由 testkit 维护）。
//!
//! ContextWindowPort / ContextCompactPort / ContextAppendPort / ReminderControlPort /
//! SessionManagementPort / MemoryOpener 实现权归 context crate，跨 crate 测试经本
//! 模块获得替身；消费方经 `[dev-dependencies] testkit` 引用，不进生产依赖图。
//!
//! 两件套：
//! - [`recording_context_over`]：装饰器——包装任意 inner `ContextPorts`，
//!   对持久化类调用（compact / append 家族 / reminder_handle_event）记录
//!   [`RecordedContextCall`] 后转发；用于验证「哪些持久化调用实际发生」。
//! - [`StubContextService`]：独立响应替身——默认成功固定响应，
//!   `compact_outcome` 可配置（take 一次）、`compact_calls` 记录压缩请求、
//!   `unavailable` 模式恒 Err（fakes 场景）。
//!
//! 会话管理侧保留 `InMemoryTestOpener` / `UnavailableSessionManagement` /
//! `ScriptedSessionManagement`；`AtomicBlobSessionManagement` 不再以类型形式迁移，
//! 消费方改走 `context::wire_session_management(blob)`。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use context::compact::{CompactResult, CompactSummaryQuality};
use context::context_port::{
    AcceptedInputAppendData, AcceptedInputError, CompactOutcome, CompactRequestData,
    CompactionDecisionData, ContextAppendData, ContextAppendError, ContextAppendPort,
    ContextCompactPort, ContextPortError, ContextPorts, ContextRequestData, ContextWindowData,
    ContextWindowPort, DecisionReason, ReminderControlPort, RunStepReceiptData,
    SessionAppendReceiptData, SessionRevision, SystemBlock, TokenBudget, Urgency,
};
use context::session::{CanonicalSession, SessionMetadata};
use context::tool_receipt::ToolCallReceiptData;
use context::{
    ReminderEventSource, ReminderKind, SessionManagementError, SessionManagementPort,
    ToolReceiptMutationData, ToolReceiptMutationError, ToolReceiptMutationReceiptData,
};
use memory::api::{MemoryOpener, MemoryOpenerError, MemoryPort, ProjectMemoryKey};
use sdk::RunId;

#[derive(Clone, Default)]
pub struct InMemoryTestOpener;

#[async_trait]
impl MemoryOpener for InMemoryTestOpener {
    async fn open_memory(
        &self,
        _key: &ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        Ok(Arc::new(
            memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default())
                .expect("default MemoryPolicy is always valid"),
        ))
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(self.clone())
    }
}

#[derive(Default)]
pub struct UnavailableSessionManagement;

#[async_trait]
impl SessionManagementPort for UnavailableSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<CanonicalSession, SessionManagementError> {
        Err(SessionManagementError::NotFound(id.to_string()))
    }

    async fn list_for_project(
        &self,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<sdk::SessionItemList>, SessionManagementError> {
        Ok(Vec::new())
    }

    async fn export_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<u8>, SessionManagementError> {
        Err(SessionManagementError::NotFound(id.to_string()))
    }

    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        Err(SessionManagementError::Storage(
            "test session port unavailable".to_string(),
        ))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
        _update: SessionMetadata,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        Err(SessionManagementError::NotFound(id.to_string()))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        Err(SessionManagementError::NotFound(id.to_string()))
    }
}

/// 会话管理替身（#2146 实现权归 context crate）：load 按脚本产出，
/// update_metadata 记录并按脚本回执；其余通道恒 Err(unused)。
pub struct ScriptedSessionManagement {
    load: Box<dyn Fn(&str) -> CanonicalSession + Send + Sync>,
    update_result: Box<dyn Fn() -> sdk::SessionItemList + Send + Sync>,
    updates: std::sync::Mutex<Vec<SessionMetadata>>,
}

impl ScriptedSessionManagement {
    pub fn new(load: impl Fn(&str) -> CanonicalSession + Send + Sync + 'static) -> Self {
        Self {
            load: Box::new(load),
            update_result: Box::new(|| sdk::SessionItemList {
                id: "session-1".to_string(),
                title: None,
                project: None,
                model: None,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
                message_count: 0,
                preview: None,
                summary: String::new(),
            }),
            updates: std::sync::Mutex::new(Vec::new()),
        }
    }
    pub fn updates(&self) -> Vec<SessionMetadata> {
        self.updates.lock().unwrap().clone()
    }
}

#[async_trait]
impl SessionManagementPort for ScriptedSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<CanonicalSession, SessionManagementError> {
        Ok((self.load)(id))
    }
    async fn list_for_project(
        &self,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<sdk::SessionItemList>, SessionManagementError> {
        Ok(Vec::new())
    }
    async fn export_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<u8>, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }
    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }
    async fn update_metadata_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentityData,
        update: SessionMetadata,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        self.updates.lock().unwrap().push(update);
        Ok((self.update_result)())
    }
    async fn delete_for_project(
        &self,
        _id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        Err(SessionManagementError::Storage("unused".to_owned()))
    }
}

// ─── 四端口装饰器 / 独立响应替身 ────────────────────────────────

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
            context::CompactTrigger::Automatic => "automatic",
            context::CompactTrigger::Manual => "manual",
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
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
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
    fn create_reminder_pipeline(&self, _run_id: sdk::RunId, _allowed: Vec<ReminderKind>) {}
    fn drop_reminder_pipeline(&self, run_id: &RunId) {
        self.inner.reminders.drop_reminder_pipeline(run_id)
    }
    fn reminder_run_started(&self, run_id: &RunId) {
        self.inner.reminders.reminder_run_started(run_id)
    }
    fn reminder_handle_event(&self, run_id: &RunId, event_source: &ReminderEventSource) {
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
    ) -> Result<ContextWindowData, ContextPortError> {
        self.record("build_window");
        if self.fail_all {
            return Err(self.err());
        }
        if self.fail_read_only {
            return Err(ContextPortError::SessionRepository(self.message.clone()));
        }
        Ok(ContextWindowData {
            backing_revision: SessionRevision::new(1),
            system_blocks: vec![SystemBlock {
                kind: "system_prompt".to_string(),
                content: "system".to_string(),
                cacheable: true,
                cache_break: true,
            }],
            messages: request.pending_messages.clone().into(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget {
                system_tokens: 2,
                tool_schema_tokens: 3,
                message_tokens: 5,
                total_tokens: 10,
            },
            compaction_decision: CompactionDecisionData {
                needed: false,
                urgency: Urgency::None,
                decision_token_count: 0,
                threshold: 1,
                context_size: 200_000,
                effective_window: 180_000,
                reason: DecisionReason::HeuristicFallback,
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
            urgency: Urgency::Must,
            decision_token_count: 100,
            threshold: 90,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::ActualProviderUsage,
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
        Ok(CompactOutcome::Committed(CompactResult {
            summary: "summary".to_string(),
            recent_messages: request.source.pending_messages.clone(),
            source_revision: SessionRevision::new(1),
            quality: CompactSummaryQuality::LocalOnly,
            metrics: Box::default(),
        }))
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
        mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        self.record("advance_tool_receipt");
        if self.fail_all {
            return Err(ToolReceiptMutationError::Storage(self.message.clone()));
        }
        self.receipt_states
            .lock()
            .unwrap()
            .push(format!("{:?}", mutation.next));
        Ok(ToolReceiptMutationReceiptData {
            receipt: ToolCallReceiptData {
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

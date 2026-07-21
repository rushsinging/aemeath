//! Context Management 领域策略、Published Language 与内部能力。

pub mod compact;
pub(crate) mod context_decision;
pub mod session;
pub(crate) mod token_budget;

pub use compact::CompactStage;
pub use token_budget::{
    autocompact_threshold, effective_context_window, estimate_message_tokens,
    estimate_messages_tokens, estimate_tokens, estimate_tool_schemas_tokens,
};

use std::collections::HashMap;

use provider::{ModelToolSchema, ReasoningLevel};
use sdk::RunId;
pub use sdk::{RunStepId, SessionId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::AgentRoleConfig;
pub use share::message::Message as ContextMessage;

macro_rules! string_value_object {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_value_object!(ContextRequestId);
string_value_object!(CalendarDate);
string_value_object!(Language);
string_value_object!(SystemPromptSpec);
string_value_object!(ContentFingerprint);

/// Session backing 的单调 revision。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionRevision(u64);

impl SessionRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Task BC 提供给 Context 的稳定只读提醒投影。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskReminderSnapshot {
    pub text: Option<String>,
}

/// 构建 window 的不可变输入；历史由 Context backing 独占。
#[derive(Debug, Clone)]
pub struct ContextRequest {
    pub session_id: SessionId,
    pub request_id: ContextRequestId,
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub pending_messages: Vec<ContextMessage>,
    pub system_prompt: SystemPromptSpec,
    pub model_id: String,
    pub effective_reasoning: ReasoningLevel,
    pub current_date: CalendarDate,
    pub task_reminder: TaskReminderSnapshot,
    pub language: Language,
    pub agent_roles: HashMap<String, AgentRoleConfig>,
    pub config_snapshot: ConfigSnapshot,
    pub context_size: usize,
    pub max_output_tokens: usize,
    pub last_api_input_tokens: Option<u64>,
    pub tool_schemas: Vec<ModelToolSchema>,
    pub tool_schema_tokens: usize,
    pub prev_system_tokens: Option<usize>,
    pub prev_tool_schema_tokens: Option<usize>,
}

/// Context-owned system block；不是任何 Provider wire DTO。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemBlock {
    pub kind: String,
    pub content: String,
    pub cacheable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenBudget {
    pub system_tokens: usize,
    pub tool_schema_tokens: usize,
    pub message_tokens: usize,
    pub total_tokens: usize,
}

/// Context window 及同一冻结输入上计算的压缩决策。
#[derive(Debug, Clone)]
pub struct ContextWindow {
    pub backing_revision: SessionRevision,
    pub system_blocks: Vec<SystemBlock>,
    pub messages: Vec<ContextMessage>,
    pub tool_schemas: Vec<ModelToolSchema>,
    pub token_estimation: TokenBudget,
    pub compaction_decision: CompactionDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    None,
    Monitor,
    Should,
    Must,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionReason {
    ActualApiWithDelta,
    Heuristic,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionDecision {
    pub needed: bool,
    pub urgency: Urgency,
    pub estimated_tokens: usize,
    pub threshold: usize,
    pub reason: DecisionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactTrigger {
    Automatic,
    Manual,
}

#[derive(Debug, Clone)]
pub struct CompactRequest {
    pub run_id: RunId,
    pub source_revision: SessionRevision,
    pub source: ContextRequest,
    pub trigger: CompactTrigger,
}

#[derive(Debug, Clone)]
pub struct ManualCompactRequest {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub system_prompt: SystemPromptSpec,
    pub context_size: usize,
}

#[derive(Debug, Clone)]
pub struct CompactResult {
    pub summary: String,
    pub recent_messages: Vec<ContextMessage>,
    pub source_revision: SessionRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactSkipReason {
    ResumeProtection,
    HookBlocked,
    CircuitBreakerOpen,
}

#[derive(Debug, Clone)]
pub enum CompactOutcome {
    Committed(CompactResult),
    Skipped(CompactSkipReason),
}

/// finalized projection 的收口原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeCause {
    Completed,
    UserCancelledStep,
    RunTerminated,
}

/// Tool/Agent 调用已经收敛的稳定结果种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOutcomeKind {
    Success,
    Failure,
    Denied,
    Cancelled,
    CancellationUnconfirmed,
}

/// finalized Step 中可确定重放的 Tool/Agent receipt。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReceipt {
    call_id: String,
    index: usize,
    outcome: ToolOutcomeKind,
    agent: bool,
    summary: Option<String>,
    artifact_refs: Vec<String>,
    possible_side_effects: Vec<String>,
    unfinished_call_ids: Vec<String>,
}

impl StepReceipt {
    pub fn tool(call_id: impl Into<String>, index: usize, outcome: ToolOutcomeKind) -> Self {
        Self::new(call_id, index, outcome, false)
    }

    pub fn agent(call_id: impl Into<String>, index: usize, outcome: ToolOutcomeKind) -> Self {
        Self::new(call_id, index, outcome, true)
    }

    fn new(
        call_id: impl Into<String>,
        index: usize,
        outcome: ToolOutcomeKind,
        agent: bool,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            index,
            outcome,
            agent,
            summary: None,
            artifact_refs: Vec::new(),
            possible_side_effects: Vec::new(),
            unfinished_call_ids: Vec::new(),
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_artifact_ref(mut self, artifact_ref: impl Into<String>) -> Self {
        self.artifact_refs.push(artifact_ref.into());
        self
    }

    pub fn with_possible_side_effect(mut self, effect: impl Into<String>) -> Self {
        self.possible_side_effects.push(effect.into());
        self
    }

    pub fn with_unfinished_call(mut self, call_id: impl Into<String>) -> Self {
        self.unfinished_call_ids.push(call_id.into());
        self
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub const fn index(&self) -> usize {
        self.index
    }

    pub const fn outcome(&self) -> ToolOutcomeKind {
        self.outcome
    }

    pub const fn is_agent(&self) -> bool {
        self.agent
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    pub fn artifact_refs(&self) -> &[String] {
        &self.artifact_refs
    }

    pub fn possible_side_effects(&self) -> &[String] {
        &self.possible_side_effects
    }

    pub fn unfinished_call_ids(&self) -> &[String] {
        &self.unfinished_call_ids
    }
}

/// 已绑定到 RunStep 的不可变 user 输入提交载荷。
#[derive(Debug, Clone)]
pub struct AcceptedInputAppend {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub source_request_id: ContextRequestId,
    pub messages: Vec<ContextMessage>,
    pub fingerprint: ContentFingerprint,
}

/// accepted input durable commit 的确定性回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedInputReceipt {
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub committed_revision: SessionRevision,
    pub fingerprint: ContentFingerprint,
}

/// accepted input append 的 typed failure。
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AcceptedInputError {
    #[error("RunStep 已接受输入冲突：run={run_id} step={step_id}")]
    ContentConflict { run_id: RunId, step_id: RunStepId },
    #[error("Session 不存在：{0}")]
    SessionNotFound(SessionId),
    #[error("Session 已接受输入持久化失败：{0}")]
    Storage(String),
}

/// 单个 finalized RunStep 的不可变提交载荷。
#[derive(Debug, Clone)]
pub struct ContextAppend {
    pub session_id: SessionId,
    pub expected_revision: SessionRevision,
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub source_request_id: ContextRequestId,
    pub finalize_cause: FinalizeCause,
    pub messages: Vec<ContextMessage>,
    pub receipts: Vec<StepReceipt>,
    pub api_input_tokens: Option<u64>,
    pub fingerprint: ContentFingerprint,
}

/// append durable commit 的确定性回执。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendReceipt {
    pub run_id: RunId,
    pub step_id: RunStepId,
    pub committed_revision: SessionRevision,
    pub fingerprint: ContentFingerprint,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ContextAppendError {
    #[error("Session revision 冲突：期望 {expected:?}，实际 {actual:?}")]
    RevisionConflict {
        expected: SessionRevision,
        actual: SessionRevision,
    },
    #[error("RunStep 内容冲突：run={run_id} step={step_id}")]
    ContentConflict { run_id: RunId, step_id: RunStepId },
    #[error("Session 不存在：{0}")]
    SessionNotFound(SessionId),
    #[error("Session 持久化失败：{0}")]
    Storage(String),
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PromptMaterializationError {
    #[error("Skill supplier materialization failed: {0}")]
    SkillSupplier(tools::SkillError),
    #[error("Baseline prompt block failure: {0}")]
    Baseline(String),
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ContextPortError {
    #[error("Session 不存在：{0}")]
    SessionNotFound(SessionId),
    #[error("Session backing 读取失败：{0}")]
    SessionRepository(String),
    #[error("Prompt 物化失败：{0}")]
    PromptMaterialization(PromptMaterializationError),
    #[error("Memory 物化失败：{0}")]
    MemoryMaterialization(String),
    #[error("Context 压缩失败：{0}")]
    Compact(String),
}

//! TUI-owned event language for Runtime stream observations.
//!
//! This module deliberately contains no SDK or runtime resource types. The
//! adapter converter is the only boundary allowed to construct these values.

use super::runtime_view::{TuiChatMessage, TuiToolResultImage};
use crate::tui::model::conversation::interaction::{UiInteractionRequestId, UiRunId, UiRunStepId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiTurnContext {
    pub(crate) chat_id: String,
    pub(crate) turn_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiToolCallStatus {
    PendingArgs,
    Ready,
    Running,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiToolCallImage {
    pub(crate) base64: String,
    pub(crate) media_type: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiRunTerminationReason {
    UserExit,
    DoubleCtrlC,
    QuitCommand,
    ProcessSignal,
    SessionShutdown,
    ParentStepCancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiRunEvent {
    Started,
    AwaitingUser,
    Resumed,
    Cancelling,
    Cancelled,
    Completed {
        result: String,
    },
    Failed {
        error: String,
    },
    Stuck {
        reason: String,
    },
    DrainingInput,
    TerminationRequested {
        reason: TuiRunTerminationReason,
        deadline_unix_millis: u64,
    },
    Terminated {
        reason: TuiRunTerminationReason,
    },
    Transitioned {
        status: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiRunStepEvent {
    Started,
    Completed,
    CancellationRequested,
    FinalizationStarted,
    Cancelled { confirmed: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiInteractionRequest {
    pub(crate) request_id: UiInteractionRequestId,
    pub(crate) run_id: UiRunId,
    pub(crate) body: TuiInteractionBody,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiInteractionBody {
    UserQuestions(Vec<TuiUserQuestion>),
    ToolApproval(TuiToolApprovalPrompt),
    PlanApproval(TuiPlanApprovalPrompt),
    HardPause(TuiStuckDiagnostic),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiUserQuestion {
    pub(crate) prompt: String,
    pub(crate) options: Vec<String>,
    pub(crate) allow_multi: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiToolApprovalPrompt {
    pub(crate) tool_name: String,
    pub(crate) args_summary: String,
    pub(crate) risk_level: TuiRiskLevel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiPlanApprovalPrompt {
    pub(crate) plan_title: String,
    pub(crate) steps: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiStuckDiagnostic {
    pub(crate) reason: String,
    pub(crate) recent_actions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiWorkspaceSnapshot {
    pub(crate) path_base: String,
    pub(crate) workspace_root: String,
    pub(crate) context_stack: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiSessionResumeFailureKind {
    NotFound,
    Corrupt,
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiReflectionTrigger {
    Interval,
    PreCompact,
    Manual,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiReflectionStatus {
    Running,
    Succeeded,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiReflectionApplyStatus {
    NotApplied,
    Applied,
    PartiallyApplied,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiReflectionErrorCategory {
    LlmCall,
    EmptyResponse,
    Parse,
    InvalidSuggestion,
    Apply,
    History,
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiReflectionRecord {
    pub(crate) id: String,
    pub(crate) timestamp: u64,
    pub(crate) trigger: TuiReflectionTrigger,
    pub(crate) status: TuiReflectionStatus,
    pub(crate) deviations: usize,
    pub(crate) suggestions: usize,
    pub(crate) outdated: usize,
    pub(crate) apply_status: TuiReflectionApplyStatus,
    pub(crate) error_category: Option<TuiReflectionErrorCategory>,
    pub(crate) token_usage: Option<(u32, u32)>,
    pub(crate) duration_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiModelSummary {
    pub(crate) provider: String,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) context_window: usize,
    pub(crate) max_tokens: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiReminder {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) done: bool,
    pub(crate) created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiSessionSummary {
    pub(crate) id: String,
    pub(crate) title: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) message_count: usize,
    pub(crate) preview: Option<String>,
    pub(crate) summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiProjectInfo {
    pub(crate) cwd: String,
    pub(crate) path_base: String,
    pub(crate) workspace_root: String,
    pub(crate) git_branch: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiConfigField {
    Model,
    PermissionMode,
    Memory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiConfigChangeCause {
    ClientUpdate,
    ProjectCommit,
    FileReload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiConfigView {
    pub(crate) model_name: String,
    pub(crate) provider: Option<String>,
    pub(crate) has_api_key: bool,
    pub(crate) permission_mode: String,
    pub(crate) markdown: bool,
    pub(crate) verbose: bool,
    pub(crate) context_size: usize,
    pub(crate) logging_level: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiHookStatus {
    Running,
    Succeeded,
    Blocked,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiHookResult {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) decision: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) additional_context: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiHookEvent {
    pub(crate) hook_name: String,
    pub(crate) status: TuiHookStatus,
    pub(crate) matcher: Option<String>,
    pub(crate) command: Option<String>,
    pub(crate) result: Option<TuiHookResult>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiHookMessageKind {
    AdditionalContext,
    SystemMessage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiHookMessage {
    pub(crate) point: String,
    pub(crate) source: String,
    pub(crate) execution_ordinal: u32,
    pub(crate) attempt: u8,
    pub(crate) kind: TuiHookMessageKind,
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TuiAgentProgressKind {
    Started { role: Option<String>, model: String },
    Message { text: String },
    ToolCalls { calls: Vec<TuiAgentToolCall> },
    ToolOutput { tool_name: String, text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TuiAgentToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) input: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TuiAgentProgress {
    pub(crate) sequence: usize,
    pub(crate) kind: TuiAgentProgressKind,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TuiRuntimeEvent {
    Text {
        context: TuiTurnContext,
        text: String,
    },
    Thinking {
        context: TuiTurnContext,
        text: String,
    },
    BlockComplete {
        context: TuiTurnContext,
        text: String,
    },
    ToolCallStart {
        context: TuiTurnContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: TuiTurnContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: TuiToolCallStatus,
    },
    ToolResult {
        context: TuiTurnContext,
        id: String,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<TuiToolResultImage>,
    },
    SystemMessage(String),
    ModelStreamWaiting {
        context: TuiTurnContext,
        elapsed_secs: u64,
        phase: String,
    },
    ModelInvocationRetrying {
        context: TuiTurnContext,
        attempt: u32,
        delay_ms: u128,
    },
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    Error(String),
    TurnStarted {
        messages: Vec<TuiChatMessage>,
    },
    MicrocompactDone {
        messages: Vec<TuiChatMessage>,
        cleared_count: usize,
    },
    StopHookBlocked {
        messages: Vec<TuiChatMessage>,
    },
    PostToolExecutionSync {
        messages: Vec<TuiChatMessage>,
    },
    ApiError {
        messages: Vec<TuiChatMessage>,
        error: String,
    },
    CompactRollback {
        messages: Vec<TuiChatMessage>,
    },
    CompactFinished {
        messages: Vec<TuiChatMessage>,
    },
    UserMessagesAdopted {
        items: Vec<TuiChatMessage>,
        queued: Vec<TuiChatMessage>,
    },
    UserMessagesQueued {
        queued: Vec<TuiChatMessage>,
    },
    Done {
        context: TuiTurnContext,
        duration_ms: Option<u64>,
    },
    Run {
        run_id: UiRunId,
        parent_run_id: Option<UiRunId>,
        event: TuiRunEvent,
    },
    RunStep {
        run_id: UiRunId,
        parent_run_id: Option<UiRunId>,
        step_id: UiRunStepId,
        event: TuiRunStepEvent,
    },
    InteractionRequested(TuiInteractionRequest),
    HookEvent(TuiHookEvent),
    HookMessage(TuiHookMessage),
    AgentProgress {
        context: TuiTurnContext,
        tool_id: String,
        event: TuiAgentProgress,
    },
    Cancelled {
        context: TuiTurnContext,
    },
    LiveTps(f64),
    TurnChanged(usize),
    WorkspaceSnapshot(TuiWorkspaceSnapshot),
    SessionReset,
    UserMessagesWithdrawn {
        texts: Vec<String>,
    },
    GraphPhaseChanged {
        node: String,
        effort: String,
        previous: String,
    },
    CompactProgress {
        stage: String,
        current: Option<u32>,
        total: Option<u32>,
    },
    ThinkingChanged {
        enabled: bool,
    },
    CommandResultText {
        text: String,
        is_error: bool,
    },
    ModelSwitched {
        display_name: String,
        context_window: usize,
        reasoning_active: Option<bool>,
    },
    ContextEstimated {
        estimated_tokens: usize,
        system_tokens: usize,
        context_size: usize,
        usage_percentage: f64,
        message_count: usize,
    },
    SessionResumed {
        messages: Vec<TuiChatMessage>,
        session_id: String,
        created_at: u64,
    },
    SessionResumeFailed {
        kind: TuiSessionResumeFailureKind,
        id: String,
        message: String,
    },
    ReflectionHistory {
        records: Vec<TuiReflectionRecord>,
    },
    ModelList {
        models: Vec<TuiModelSummary>,
    },
    ReminderList {
        reminders: Vec<TuiReminder>,
    },
    SessionList {
        sessions: Vec<TuiSessionSummary>,
    },
    ProjectInfo {
        project: TuiProjectInfo,
    },
    TasksSnapshot {
        lines: Vec<String>,
    },
    CostUpdate {
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
    },
    ConfigChanged {
        cause: TuiConfigChangeCause,
        changed_fields: Vec<TuiConfigField>,
        view: TuiConfigView,
    },
    ConfigReloaded {
        changed_keys: Vec<String>,
    },
}

#[cfg(test)]
#[path = "tui_runtime_event_tests.rs"]
mod tests;

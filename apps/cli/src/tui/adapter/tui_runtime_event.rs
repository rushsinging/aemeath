//! TUI-owned event language for Runtime stream observations.
//!
//! This module deliberately contains no SDK or runtime resource types. The
//! adapter converter is the only boundary allowed to construct these values.
//!
//! Some structs are not yet exercised by production; retained as DTO reserves
//! for #1246 / #944 5B.

#![allow(dead_code)]

use super::runtime_view::{TuiChatMessage, TuiToolResultImage};
use crate::tui::model::conversation::interaction::{UiInteractionRequestId, UiRunId, UiRunStepId};
use crate::tui::view_model::markdown_spacing::MarkdownSpacingPolicy;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct UiActivityId(String);

impl From<&str> for UiActivityId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl UiActivityId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivityChangeKind {
    Started,
    Updated,
    Finished,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivitySource {
    Run,
    RunStep(UiRunStepId),
    ModelInvocation(String),
    ToolCall(String),
    HookDispatch(UiActivityId),
    Compaction(UiActivityId),
    Interaction(String),
    ChildRun(UiRunId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiRunPhaseKind {
    DrainingInput,
    PreparingContext,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    FinalizingStep,
    CancellingStep,
    Terminating,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivityKind {
    Run,
    RunPhase(TuiRunPhaseKind),
    ModelInvocation,
    ToolCall,
    HookDispatch,
    Compaction,
    Interaction,
    ChildRun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivityState {
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Terminated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivityAudience {
    User,
    Operational,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiRunPurpose {
    Main,
    Derived,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiModelStreamState {
    Invoking,
    WaitingForFirstToken,
    Streaming,
    Retrying,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiHookPoint {
    PreToolUse,
    UserPromptSubmit,
    PreCompact,
    PermissionRequest,
    Elicitation,
    UserPromptExpansion,
    Stop,
    PostToolUse,
    PostToolUseFailure,
    PostCompact,
    PostToolBatch,
    ElicitationResult,
    SessionStart,
    SessionEnd,
    SubRunStart,
    SubRunStop,
    TaskCreated,
    TaskCompleted,
    Notification,
    InstructionsLoaded,
    StopFailure,
    PermissionDenied,
    ConfigChange,
    CwdChanged,
    FileChanged,
    TeammateIdle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiCompactStage {
    Preparing,
    Summarizing,
    Finalizing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TuiInteractionKind {
    ToolApproval,
    UserQuestion,
    PlanApproval,
    StuckDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TuiActivityDetail {
    Run {
        purpose: TuiRunPurpose,
    },
    Phase {
        phase: TuiRunPhaseKind,
    },
    Model {
        model: String,
        attempt: u32,
        stream: TuiModelStreamState,
    },
    Tool {
        name: String,
        summary: Option<String>,
        parallel_count: u16,
    },
    Hook {
        point: TuiHookPoint,
        script: String,
        attempt: u8,
    },
    Compact {
        stage: TuiCompactStage,
        current: Option<u32>,
        total: Option<u32>,
    },
    Interaction {
        kind: TuiInteractionKind,
    },
    ChildRun {
        role: String,
        model: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TuiActivityTiming {
    pub(crate) total_elapsed_ms: u64,
    pub(crate) active_elapsed_ms: u64,
    pub(crate) state_elapsed_ms: u64,
    pub(crate) started_at_unix_ms: Option<u64>,
    pub(crate) finished_at_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiActivityObservation {
    pub(crate) id: UiActivityId,
    pub(crate) run_id: UiRunId,
    pub(crate) run_step_id: Option<UiRunStepId>,
    pub(crate) parent_activity_id: Option<UiActivityId>,
    pub(crate) source: TuiActivitySource,
    pub(crate) kind: TuiActivityKind,
    pub(crate) state: TuiActivityState,
    pub(crate) detail: TuiActivityDetail,
    pub(crate) audience: TuiActivityAudience,
    pub(crate) revision: u64,
    pub(crate) timing: TuiActivityTiming,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiActivitySnapshot {
    pub(crate) run_id: UiRunId,
    pub(crate) revision: u64,
    pub(crate) activities: Vec<TuiActivityObservation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiRunContext {
    pub(crate) chat_id: String,
    pub(crate) run_id: String,
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
    pub(crate) tool_call_id: Option<String>,
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
    pub(crate) markdown_spacing: MarkdownSpacingPolicy,
    pub(crate) context_size: usize,
    pub(crate) logging_level: String,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiSkillView {
    pub(crate) name: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) slash_command: Option<String>,
    pub(crate) slash_aliases: Vec<String>,
    pub(crate) description: String,
    pub(crate) argument_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TuiSkillSlashRoute {
    pub(crate) skill: String,
    pub(crate) slash_command: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) argument_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TuiRuntimeEvent {
    Noop,
    ActivityChanged {
        kind: TuiActivityChangeKind,
        activity: TuiActivityObservation,
    },
    ActivitySnapshot(TuiActivitySnapshot),
    SkillsUpdated {
        revision: String,
        skills: Vec<TuiSkillView>,
        slash_routes: Vec<TuiSkillSlashRoute>,
    },
    Text {
        context: TuiRunContext,
        text: String,
    },
    Thinking {
        context: TuiRunContext,
        text: String,
    },
    BlockComplete {
        context: TuiRunContext,
        text: String,
    },
    ToolCallStart {
        context: TuiRunContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: TuiRunContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: TuiToolCallStatus,
    },
    ToolResult {
        context: TuiRunContext,
        id: String,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<TuiToolResultImage>,
    },
    SystemMessage(String),
    ModelInvocationRetrying {
        context: TuiRunContext,
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
    SessionMessageStateChanged {
        message_count: usize,
        revision: u64,
    },
    StopHookFeedback(crate::tui::adapter::runtime_view::TuiStopHookFeedback),
    ApiError {
        messages: Vec<TuiChatMessage>,
        error: String,
    },
    CompactRollback {
        messages: Vec<TuiChatMessage>,
    },
    CompactFinished {
        messages: Vec<TuiChatMessage>,
        notice: String,
    },
    UserMessagesAdopted {
        items: Vec<TuiChatMessage>,
        queued: Vec<TuiChatMessage>,
    },
    UserMessagesQueued {
        queued: Vec<TuiChatMessage>,
    },
    Done {
        context: TuiRunContext,
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
    AgentProgress {
        source_context: TuiRunContext,
        attachment_context: TuiRunContext,
        tool_id: String,
        event: TuiAgentProgress,
    },
    Cancelled {
        context: TuiRunContext,
        duration_ms: u64,
    },
    LiveTps(f64),
    RunChanged(usize),
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
        steps: Vec<super::runtime_view::TuiResumedSessionStep>,
        display_history: Option<super::runtime_view::TuiDisplayHistoryIndex>,
        session_id: String,
        created_at: u64,
        compacted: bool,
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
        view: TuiConfigView,
    },
}

#[cfg(test)]
#[path = "tui_runtime_event_tests.rs"]
mod tests;

//! Runtime Activity 观测的 SDK Published Language。
//!
//! 本模块只包含客户端无关的完整事实值，不包含 TUI 文案、颜色、布局或原始 payload。

pub use crate::ids::ActivityId;
use crate::{
    InteractionRequestId, ModelInvocationId, ReflectionTriggerView, RunId, RunStepId, ToolCallId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityChangeKind {
    Started,
    Updated,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ActivitySourceView {
    Run,
    RunStep(RunStepId),
    ModelInvocation(ModelInvocationId),
    ToolCall(ToolCallId),
    HookDispatch(ActivityId),
    Compaction(ActivityId),
    Reflection(ActivityId),
    Interaction(InteractionRequestId),
    SubRun(RunId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunPhaseKindView {
    DrainingInput,
    PreparingContext,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    FinalizingStep,
    CancellingStep,
    Terminating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ActivityKindView {
    Run,
    RunPhase(RunPhaseKindView),
    ModelInvocation,
    ToolCall,
    HookDispatch,
    Compaction,
    Reflection,
    Interaction,
    SubRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStateView {
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityAudienceView {
    User,
    Operational,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunPurposeView {
    Main,
    Derived,
    Reflection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelStreamStateView {
    Invoking,
    WaitingForFirstToken,
    Streaming,
    Retrying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookPointView {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactStageView {
    Preparing,
    Generating,
    Mapping,
    Reducing,
    Refreshing,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "work_type")]
pub enum CompactWorkView {
    Indeterminate,
    Determinate { completed: u32, total: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKindView {
    ToolApproval,
    UserQuestion,
    StuckDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "detail_type")]
pub enum ActivityDetailView {
    Run {
        purpose: RunPurposeView,
    },
    Phase {
        phase: RunPhaseKindView,
    },
    Model {
        model: String,
        attempt: u32,
        stream: ModelStreamStateView,
    },
    Tool {
        name: String,
        summary: Option<String>,
        parallel_count: u16,
    },
    Hook {
        point: HookPointView,
        script: String,
        attempt: u8,
    },
    Compact {
        stage: CompactStageView,
        work: CompactWorkView,
    },
    Reflection {
        trigger: ReflectionTriggerView,
    },
    Interaction {
        kind: InteractionKindView,
    },
    SubRun {
        role: String,
        model: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityTimingView {
    pub total_elapsed_ms: u64,
    pub active_elapsed_ms: u64,
    pub state_elapsed_ms: u64,
    pub started_at_unix_ms: Option<u64>,
    pub finished_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityView {
    pub id: ActivityId,
    pub run_id: RunId,
    pub run_step_id: Option<RunStepId>,
    pub parent_activity_id: Option<ActivityId>,
    pub source: ActivitySourceView,
    pub kind: ActivityKindView,
    pub state: ActivityStateView,
    pub detail: ActivityDetailView,
    pub audience: ActivityAudienceView,
    pub revision: u64,
    pub timing: ActivityTimingView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActivitySnapshotView {
    pub run_id: RunId,
    pub revision: u64,
    #[serde(default)]
    pub heartbeat_sequence: u64,
    pub activities: Vec<ActivityView>,
}

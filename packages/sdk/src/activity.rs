//! Runtime Activity 观测的 SDK Published Language。
//!
//! 本模块只包含客户端无关的完整事实值，不包含 TUI 文案、颜色、布局或原始 payload。

use crate::{InteractionRequestId, ModelInvocationId, RunId, RunStepId, ToolCallId};
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Runtime-owned Activity identity (UUIDv7)。
#[derive(Debug, Clone)]
pub struct ActivityId(Uuid, String);

impl ActivityId {
    pub fn new_v7() -> Self {
        let uuid = Uuid::now_v7();
        Self(uuid, uuid.to_string())
    }

    pub fn new(value: impl AsRef<str>) -> Self {
        let value = value.as_ref();
        if let Ok(uuid) = Uuid::parse_str(value) {
            if uuid.get_version_num() == 7 {
                return Self(uuid, uuid.to_string());
            }
        }
        let namespace = Uuid::from_bytes([
            0xa1, 0xc7, 0x1a, 0x17, 0x1a, 0x17, 0x1a, 0x17, 0xa1, 0xc7, 0x1a, 0x17, 0x1a, 0x17,
            0x1a, 0x17,
        ]);
        let base = Uuid::new_v5(&namespace, value.as_bytes());
        let mut bytes = *base.as_bytes();
        bytes[6] = (bytes[6] & 0x0f) | 0x70;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let uuid = Uuid::from_bytes(bytes);
        Self(uuid, uuid.to_string())
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        &self.1
    }
}

impl PartialEq for ActivityId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for ActivityId {}

impl Hash for ActivityId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl fmt::Display for ActivityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.1)
    }
}

impl AsRef<str> for ActivityId {
    fn as_ref(&self) -> &str {
        &self.1
    }
}

impl Serialize for ActivityId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ActivityId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let uuid = Uuid::deserialize(deserializer)?;
        if uuid.get_version_num() != 7 {
            return Err(serde::de::Error::custom(format!(
                "Activity UUID 不是 version 7: {uuid}"
            )));
        }
        Ok(Self(uuid, uuid.to_string()))
    }
}

impl JsonSchema for ActivityId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ActivityId".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "format": "uuid",
            "x-aemeath-uuid-version": 7
        })
    }
}

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
    PlanApproval,
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

//! AgentClient SDK — CLI 与 Agent Runtime 之间的唯一通信契约。
//!
//! `packages/sdk` 只放 trait + 公共类型，零业务依赖。
//! 实现在 `agent/runtime`。

pub mod activity;
#[cfg(test)]
#[path = "activity_tests.rs"]
mod activity_tests;
pub mod bootstrap;
pub mod change_set;
pub mod chat;
mod chat_event;
mod chat_result;
mod chat_view;
pub mod client;
pub mod commands;
pub mod config_view;
pub mod content;
pub mod error;
pub mod models;
pub mod project;
pub mod run;
pub mod session;
pub mod session_lock;

/// 会话恢复失败分类（#636 D2）。顶层 re-export 方便 runtime / CLI 直接引用。
pub use chat_event::{
    DisplayHistoryIndex, DisplayHistoryStepReference, DisplayHistoryWindow,
    DisplayHistoryWindowRequest, LocalResumedSessionStep, LocalSessionResumeBacking,
    SessionResumeFailureKind, SessionResumeView,
};
pub mod task;
pub mod tool_input;
pub mod tool_result;
pub mod tui;
pub mod types;
pub mod update;
pub mod wire;

pub mod ids;
pub mod interaction;
mod runtime_status;

pub use activity::{
    ActivityAudienceView, ActivityChangeKind, ActivityDetailView, ActivityId, ActivityKindView,
    ActivitySnapshotView, ActivitySourceView, ActivityStateView, ActivityTimingView, ActivityView,
    CompactStageView, CompactWorkView, HookPointView, InteractionKindView, ModelStreamStateView,
    RunPhaseKindView, RunPurposeView,
};
pub use bootstrap::{ChatBootstrapArgs, LoggingOutputMode};
pub use change_set::ChangeSet;
pub use chat::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatEventContext, ChatInput, ChatInputEvent, ChatInputImage, ChatRequest, ChatResult,
    ChatStream, OptionItem, ReflectionApplyStatusView, ReflectionErrorCategoryView,
    ReflectionHistoryView, ReflectionStatusView, ReflectionTokenUsageView, ReflectionTriggerView,
    ResumedSessionStep, ResumedStepFinalizeCause, RunStatusView, RunTimingView, SkillRequest,
    SubRunActivityEventView, SubRunActivityKindView, SubRunIdentityView, SubRunStartedEventView,
    SubRunTerminalOutcomeView, ToolCallStatusView, ToolProgressEventView, ToolResultImage,
    WorkspaceContextView, WorkspaceStackEntryView,
};
pub use client::{AgentClient, DisplayHistoryQuery, RunControlClient};
pub use commands::{
    ApplicationControlCommand, ApplicationControlTarget, CommandArgumentSchema, CommandCatalogPort,
    CommandCompletion, CommandDescriptor, CommandMechanism, CommandName, CommandParseError,
    CommandRoute, CommandRouterPort, CommandTarget, ContextEstimate, ModelSwitchResult,
    ParsedArguments, SkillRequestCommand, SlashInput, SnapshotQueryCommand, SnapshotQueryTarget,
};
pub use config_view::{
    ConfigApplicationScopeView, ConfigChangeCause, ConfigChangedEvent, ConfigField,
    ConfigReloadedEvent, ConfigUpdate, ConfigUpdateResult, ConfigView, ElementSpacingView,
    MarkdownSpacingModeView, MarkdownSpacingOverridesView, PermissionModeView,
};
pub use content::{ContentBlock, ImageSource};
pub use error::SdkError;
pub use ids::{
    AgentId, ChatId, ChatRunId, IdParseError, InputId, InteractionRequestId, ModelInvocationId,
    RunId, RunStepId, SessionId, ToolCallId,
};
pub use interaction::{
    ApprovalDecision, InteractionCancelReason, InteractionCommandOutcome, InteractionReply,
    InteractionReplyError, InteractionRequest, InteractionRequestBody, PlanApprovalPrompt,
    RiskLevel, StuckDiagnostic, ToolApprovalPrompt, UserAnswer, UserQuestion,
};
pub use models::ModelSummary;
pub use project::ProjectContext;
pub use run::{
    CancelCurrentRunOutcome, CancelRunStepOutcome, ControlDeadline, RunStepCancellationTerminal,
    RunTerminationReason, TerminateRunOutcome,
};
pub use runtime_status::{ContextBudgetView, ContextDecisionSourceView, RuntimeStatusView};
pub use session::{
    ChatMessage, ChatMessageMetadata, ChatMessageSource, HookNoticeKindView, HookNoticeView,
    SessionSnapshot, SessionSummary, SkillRequestMetadataView,
};
pub use share::message::ContentBlock as LocalResumeContentBlock;
pub use share::message::{
    HookNotice as LocalResumeHookNotice, HookNoticeKind as LocalResumeHookNoticeKind,
    Message as LocalResumeMessage, MessageSource as LocalResumeMessageSource,
    Role as LocalResumeRole,
};
pub use task::{
    TaskBatchStatusView, TaskBatchView, TaskItemStatusView, TaskItemView, TaskPriorityView,
    TaskStateView,
};
pub use tui::{
    classify_paste, is_image_file_path, ChatEventSink, ChatHandle, ChatInputEventPort,
    ClipboardImageView, InputEventFuture, InputEventOptFuture, MemoryConfigView, PasteKind,
    ReflectionConfigView, ReminderView, SkillSlashRouteView, SkillView, SkillsUpdatedEvent,
    TuiLaunchContext,
};
pub use types::{
    char_to_byte, format_tokens, ByteIdx, CharIdx, CostInfo, PermissionPrompt, StatusInfo,
    StrSlice, TaskState, TaskSummary,
};
pub use update::{UpdateResult, UpdateService, VersionCheck};
pub use utils::{slice_head, slice_tail};

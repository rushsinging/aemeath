//! AgentClient SDK — CLI 与 Agent Runtime 之间的唯一通信契约。
//!
//! `packages/sdk` 只放 trait + 公共类型，零业务依赖。
//! 实现在 `agent/runtime`。

pub mod bootstrap;
pub mod change_set;
pub mod chat;
pub mod client;
pub mod commands;
pub mod error;
pub mod models;
pub mod project;
pub mod session;
pub mod tui;
pub mod types;

pub use bootstrap::ChatBootstrapArgs;
pub use change_set::ChangeSet;
pub use chat::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatEventContext, ChatInput, ChatInputEvent, ChatRequest, ChatResult, ChatStream,
    HookEventStatus, HookEventView, HookExecutionResultView, OptionItem, ToolResultImage,
    WorkspaceContextView, WorkspaceStackEntryView,
};
pub use client::AgentClient;
pub use commands::builtin_commands;
pub use commands::{
    CommandAction, CommandContext, CommandResult, ConfirmAction, ContextEstimate,
    ModelSwitchParams, ModelSwitchResult,
};
pub use error::SdkError;
pub use models::ModelSummary;
pub use project::ProjectContext;
pub use session::{ChatMessage, SessionSnapshot, SessionSummary};
pub use tui::{
    classify_paste, is_image_file_path, ChatEventSink, ChatHandle, ChatInputEventPort,
    ClipboardImageView, InputEventFuture, MemoryConfigView, PasteKind, QueueDrainPort, QueueFuture,
    ReflectionConfigView, ReflectionMemorySuggestionView, ReflectionOutputView, ReminderView,
    SkillView, TaskStatusView, TuiLaunchContext,
};
pub use types::{
    char_to_byte, format_tokens, ByteIdx, CharIdx, CostInfo, PermissionPrompt, StatusInfo,
    StrSlice, TaskState, TaskSummary,
};

//! AgentClient SDK — CLI 与 Agent Runtime 之间的唯一通信契约。
//!
//! `packages/sdk` 只放 trait + 公共类型，零业务依赖。
//! 实现在 `agent/runtime`。

pub mod bootstrap;
pub mod change_set;
pub mod chat;
mod chat_event;
mod chat_result;
mod chat_view;
pub mod client;
pub mod commands;
pub mod content;
pub mod error;
pub mod models;
pub mod project;
pub mod session;
pub mod tool_result;
pub mod tui;
pub mod types;
pub mod update;

pub mod ids;

pub use bootstrap::ChatBootstrapArgs;
pub use change_set::ChangeSet;
pub use chat::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, AskUserQuestionItem,
    ChatEvent, ChatEventContext, ChatInput, ChatInputEvent, ChatInputImage, ChatRequest,
    ChatResult, ChatStream, HookEventStatus, HookEventView, HookExecutionResultView, OptionItem,
    ToolCallStatusView, ToolResultImage, WorkspaceContextView, WorkspaceStackEntryView,
};
pub use client::AgentClient;
pub use commands::builtin_commands;
pub use commands::{
    CommandAction, CommandContext, CommandResult, ConfirmAction, ContextEstimate,
    ModelSwitchParams, ModelSwitchResult,
};
pub use content::{ContentBlock, ImageSource};
pub use error::SdkError;
pub use ids::{ChatId, ChatTurnId, IdParseError, InputId, ToolCallId};
pub use models::ModelSummary;
pub use project::ProjectContext;
pub use session::{
    ChatMessage, ChatMessageMetadata, ChatMessageSource, SessionSnapshot, SessionSummary,
};
pub use tui::{
    classify_paste, is_image_file_path, ChatEventSink, ChatHandle, ChatInputEventPort,
    ClipboardImageView, InputEventFuture, InputEventOptFuture, MemoryConfigView, PasteKind,
    QueueDrainPort, QueueFuture, ReflectionConfigView, ReflectionMemorySuggestionView,
    ReflectionOutputView, ReminderView, SkillView, TaskStatusView, TuiLaunchContext,
};
pub use types::{
    char_to_byte, format_tokens, ByteIdx, CharIdx, CostInfo, PermissionPrompt, StatusInfo,
    StrSlice, TaskState, TaskSummary,
};
pub use update::{UpdateResult, UpdateService, VersionCheck};
pub use utils::{slice_head, slice_tail};

use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use std::path::PathBuf;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UiTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl From<sdk::ChatEventContext> for UiTurnContext {
    fn from(context: sdk::ChatEventContext) -> Self {
        Self {
            chat_id: ChatId::new(context.chat_id),
            turn_id: ChatTurnId::new(context.turn_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusContextUpdate {
    pub path_base: String,
    pub working_root: String,
    pub branch: Option<String>,
    pub kind: crate::tui::model::runtime::workspace::WorktreeKind,
    pub raw_path_base: PathBuf,
    pub raw_working_root: PathBuf,
    pub workspace: sdk::WorkspaceContextView,
}

/// Events sent from background task to UI
#[derive(Debug)]
pub enum AppEvent {
    Text {
        context: UiTurnContext,
        text: String,
    },
    Thinking {
        context: UiTurnContext,
        text: String,
    },
    BlockComplete {
        context: UiTurnContext,
        text: String,
    },
    ToolCallStart {
        context: UiTurnContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: UiTurnContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        summary: Option<String>,
        status: sdk::ToolCallStatusView,
    },
    ToolResult {
        context: UiTurnContext,
        id: String,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<sdk::ToolResultImage>,
    },
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    Error(String),
    Cancelled {
        context: UiTurnContext,
    },
    MessagesSync(Vec<sdk::ChatMessage>),
    Done {
        context: UiTurnContext,
    },
    DoneWithDuration {
        context: UiTurnContext,
        duration: std::time::Duration,
    },
    LiveTps(f64),
    ClipboardImage(sdk::ClipboardImageView),
    SystemMessage(String),
    /// session reminder recap 行（每轮结束后由 run_loop 异步获取并回传）。
    ReminderRecap(String),
    /// /memory 命令的 reminder 列表回传。
    MemoryList(Vec<sdk::ReminderView>),
    /// /save 命令保存成功后回传（携带 session id），用于推送 `[session saved: id]` 反馈行。
    SessionSaved {
        id: String,
    },
    /// slash 命令副作用失败的反馈（如 /save、/memory），推送错误提示行。
    SlashCommandFailed {
        message: String,
    },
    ReflectionStarted,
    ReflectionUsage,
    ReflectionDone {
        output: sdk::ReflectionOutputView,
    },
    /// Reflection apply 完成/失败结果。携带提交时的 output，用于只清理对应 in-flight。
    ReflectionApplyDone {
        output: sdk::ReflectionOutputView,
        result: Result<String, String>,
    },
    /// AskUserQuestion tool call: pause and wait for user input
    AskUser {
        id: String,
        question: String,
        options: Vec<sdk::OptionItem>,
        #[allow(dead_code)]
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Sub-agent progress update (streams per-turn output to TUI)
    AgentProgress {
        context: UiTurnContext,
        tool_id: String,
        event: sdk::AgentProgressEventView,
    },
    /// Unified lifecycle hook event.
    HookEvent(sdk::HookEventView),
    /// Background agent loop requests queued user input before next LLM call.
    DrainQueuedInput {
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// 当前 turn 变化，需要由 CLI 边界记录到 runtime bootstrap。
    CurrentTurnChanged(usize),
    /// Current tool path base/working root changed.
    WorkingDirectoryChanged(StatusContextUpdate),
    /// Runtime task store changed; refresh TUI task list window.
    TaskStatusChanged,
}
pub type UiEvent = AppEvent;

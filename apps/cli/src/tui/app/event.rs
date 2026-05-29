use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct StatusContextUpdate {
    pub path_base: String,
    pub working_root: String,
    pub branch: Option<String>,
    pub kind: crate::tui::render::status::WorktreeKind,
    pub raw_path_base: PathBuf,
    pub raw_working_root: PathBuf,
    pub workspace: sdk::WorkspaceContextView,
}

/// Events sent from background task to UI
#[derive(Debug)]
pub enum AppEvent {
    Text(String),
    Thinking(String),
    TextBlockComplete(String),
    ToolCallStart {
        name: String,
        index: usize,
    },
    ToolArgumentsDelta {
        index: usize,
        name: String,
        partial_args: String,
    },
    ToolCall {
        id: String,
        name: String,
        index: Option<usize>,
        summary: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        output: String,
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
    Cancelled,
    MessagesSync(Vec<sdk::ChatMessage>),
    Done,
    DoneWithDuration(std::time::Duration),
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
    ReflectionUsage {
        input: u32,
        output: u32,
    },
    ReflectionDone {
        output: sdk::ReflectionOutputView,
    },
    /// AskUserQuestion tool call: pause and wait for user input
    AskUser {
        id: String,
        question: String,
        options: Vec<String>,
        #[allow(dead_code)]
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Sub-agent progress update (streams per-turn output to TUI)
    AgentProgress {
        tool_id: String,
        event: sdk::AgentProgressEventView,
    },
    StopFailureHook {
        system_message: Option<String>,
        additional_context: Option<String>,
    },
    /// Background agent loop requests queued user input before next LLM call.
    DrainQueuedInput {
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Lifecycle hook execution started.
    HookStart {
        event: String,
        command: String,
    },
    /// Lifecycle hook execution finished.
    HookEnd {
        event: String,
        blocked: bool,
        error: Option<String>,
    },
    /// 当前 turn 变化，需要由 CLI 边界记录到 runtime bootstrap。
    CurrentTurnChanged(usize),
    /// Current tool path base/working root changed.
    WorkingDirectoryChanged(StatusContextUpdate),
}
pub type UiEvent = AppEvent;

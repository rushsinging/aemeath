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
            chat_id: context.chat_id,
            turn_id: context.turn_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusContextUpdate {
    pub path_base: String,
    pub workspace_root: String,
    pub branch: Option<String>,
    pub kind: crate::tui::model::runtime::workspace::WorktreeKind,
    pub raw_path_base: PathBuf,
    pub raw_workspace_root: PathBuf,
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
        id: sdk::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: UiTurnContext,
        id: sdk::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: sdk::ToolCallStatusView,
    },
    ToolResult {
        context: UiTurnContext,
        id: sdk::ids::ToolCallId,
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
    /// Turn 启动，TUI 据此启动 spinner(Thinking)。
    TurnStarted {
        messages: Vec<sdk::ChatMessage>,
    },
    /// Microcompact 清理了陈旧 tool result，TUI 只同步消息。
    MicrocompactDone {
        messages: Vec<sdk::ChatMessage>,
        cleared_count: usize,
    },
    /// Stop hook 阻止 turn 结束，TUI 只同步消息。
    StopHookBlocked {
        messages: Vec<sdk::ChatMessage>,
    },
    /// Tool 执行完成后同步，TUI 只同步消息。
    PostToolExecutionSync {
        messages: Vec<sdk::ChatMessage>,
    },
    /// Provider API 调用失败，TUI stop spinner + 显示错误。
    ApiError {
        messages: Vec<sdk::ChatMessage>,
        error: String,
    },
    /// Compact 失败回滚，TUI 只同步消息。
    CompactRollback {
        messages: Vec<sdk::ChatMessage>,
    },
    /// Compact 成功完成，TUI 同步消息 + 清 compact 状态。
    CompactFinished {
        messages: Vec<sdk::ChatMessage>,
    },
    /// 批量用户输入归宿通知（#507 修复）。每条 ChatMessage 由 runtime 端 share::Message
    /// 映射而来，含 typed blocks + image placeholder + input_id；TUI 用 ChatMessage.input_id
    /// 清占位、ChatMessage.text_content() 还原回显（含 Image placeholder）。
    UserMessagesAdded(Vec<sdk::ChatMessage>),
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
    /// AskUserQuestion 批量请求——一次携带多个问题。
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Sub-agent progress update (streams per-turn output to TUI)
    AgentProgress {
        context: UiTurnContext,
        tool_id: sdk::ids::ToolCallId,
        event: sdk::AgentProgressEventView,
    },
    /// Unified lifecycle hook event.
    HookEvent(sdk::HookEventView),
    /// 当前 turn 变化，需要由 CLI 边界记录到 runtime bootstrap。
    CurrentTurnChanged(usize),
    /// Current tool path base/working root changed.
    WorkingDirectoryChanged(StatusContextUpdate),
    /// Runtime task store changed; refresh TUI task list window.
    TaskStatusChanged,
    /// 版本检查结果（后台 spawn 完成后回送）。
    UpdateAvailable {
        current: String,
        latest: String,
        release_url: String,
    },
    /// runtime 完成 reset 清理，TUI 据此清空镜像。
    SessionReset,
    /// 批量撤回 pending 输入（#391 S3）。texts 为被撤回文本，TUI join("\n") 还原输入框。
    UserMessagesWithdrawn(Vec<String>),
    /// Reasoning Graph 阶段变化（Phase 2）。更新 status bar 的阶段展示。
    GraphPhaseChanged {
        node: String,
    },
    /// Compact 进度更新。TUI 渲染 Gauge 进度条。
    CompactProgress {
        stage: String,
        current: Option<u32>,
        total: Option<u32>,
    },
    /// 模型切换完成（#497）。TUI 据此更新 5 个本地状态 + 回显。
    ModelSwitched {
        result: sdk::ModelSwitchResult,
    },
    /// Reasoning 模式切换完成（#497）。TUI 据此更新 thinking 状态 + 回显。
    ThinkingChanged {
        enabled: bool,
    },
    /// 上下文估算完成（#497）。TUI 据此显示 token 占用信息。
    ContextEstimated {
        estimate: sdk::ContextEstimate,
        message_count: usize,
    },
    /// 查询命令执行完成，返回纯文本结果（#497）。
    CommandResultText {
        text: String,
        is_error: bool,
    },
    /// 会话恢复完成（#497）。TUI 据此更新 messages。
    SessionResumed {
        messages: Vec<sdk::ChatMessage>,
        session_id: String,
        #[allow(dead_code)]
        created_at: u64,
    },
}
pub type UiEvent = AppEvent;

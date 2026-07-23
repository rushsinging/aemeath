//! UiEvent carries local Effect 回灌 and legacy Runtime branches.
//! After #943 阶段 3, Runtime events flow through TuiRuntimeEvent; the
//! remaining Runtime variants in UiEvent are dead code pending #944 5B.
#![allow(dead_code)]
use crate::tui::adapter::runtime_view::TuiChatMessage;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId};
use crate::tui::model::conversation::workspace::WorktreeKind;
use std::path::PathBuf;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UiTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl From<sdk::ChatEventContext> for UiTurnContext {
    fn from(context: sdk::ChatEventContext) -> Self {
        Self {
            chat_id: ChatId::new(context.chat_id.as_str()),
            turn_id: ChatTurnId::new(context.turn_id.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusContextUpdate {
    pub path_base: String,
    pub workspace_root: String,
    pub raw_path_base: PathBuf,
    pub raw_workspace_root: PathBuf,
    pub workspace: sdk::WorkspaceContextView,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WorkspaceMetadataResolved {
    pub root: String,
    pub revision: u64,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ModelStreamWaitingView {
    pub context: UiTurnContext,
    pub elapsed_secs: u64,
    pub phase: String,
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
    RunCancelled,
    Cancelled {
        context: UiTurnContext,
    },
    /// Turn 启动，TUI 据此启动 spinner(Thinking)。
    TurnStarted {
        messages: Vec<TuiChatMessage>,
    },
    /// Microcompact 清理了陈旧 tool result，TUI 只同步消息。
    MicrocompactDone {
        messages: Vec<TuiChatMessage>,
        cleared_count: usize,
    },
    /// Stop hook 阻止 turn 结束，TUI 只同步消息。
    StopHookBlocked {
        messages: Vec<TuiChatMessage>,
    },
    /// Tool 执行完成后同步，TUI 只同步消息。
    PostToolExecutionSync {
        messages: Vec<TuiChatMessage>,
    },
    /// Provider API 调用失败，TUI stop spinner + 显示错误。
    ApiError {
        messages: Vec<TuiChatMessage>,
        error: String,
    },
    /// Compact 失败回滚，TUI 只同步消息。
    CompactRollback {
        messages: Vec<TuiChatMessage>,
    },
    /// Compact 成功完成，TUI 同步消息 + 清 compact 状态。
    CompactFinished {
        messages: Vec<TuiChatMessage>,
    },
    /// 批量用户输入归宿通知（#507 修复）。每条 ChatMessage 由 runtime 端 share::Message
    /// 映射而来，含 typed blocks + image placeholder + input_id；TUI 用 ChatMessage.input_id
    /// 清占位、ChatMessage.text_content() 还原回显（含 Image placeholder）。
    /// 用户输入被 gate 接纳（idle 直发或 batch drain）。
    /// items = 本批接纳的消息；queued = 仍留在 buffer 中的排队消息（一般空）。
    UserMessagesAdopted {
        items: Vec<TuiChatMessage>,
        queued: Vec<TuiChatMessage>,
    },
    /// busy 阶段收到新输入并存入 runtime buffer 后的确认。
    /// queued = 全量 buffer 快照。TUI 据此全量重渲染 queue 区域。
    UserMessagesQueued {
        queued: Vec<TuiChatMessage>,
    },
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
    ModelStreamWaiting {
        context: UiTurnContext,
        elapsed_secs: u64,
        phase: String,
    },
    /// /save 命令保存成功后回传（携带 session id），用于推送 `[session saved: id]` 反馈行。
    SessionSaved {
        id: String,
    },
    /// Safe reflection history metadata; never contains prompts or reflection body text.
    ReflectionHistory {
        records: Vec<sdk::ReflectionHistoryView>,
    },
    /// AskUserQuestion 批量请求——一次携带多个问题。
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<sdk::AskUserReply>,
    },
    /// Sub-agent progress update (streams per-turn output to TUI)
    AgentProgress {
        context: UiTurnContext,
        tool_id: sdk::ids::ToolCallId,
        event: sdk::AgentProgressEventView,
    },
    /// Unified lifecycle hook event.
    HookEvent(sdk::HookEventView),
    /// Hook-produced context or system message for structured conversation display.
    HookMessage(sdk::HookMessageView),
    /// 当前 turn 变化，需要由 CLI 边界记录到 runtime bootstrap。
    CurrentTurnChanged(usize),
    /// Current tool path base/working root changed.
    WorkingDirectoryChanged(StatusContextUpdate),
    WorkspaceMetadataResolved(WorkspaceMetadataResolved),
    /// Runtime task store changed; refresh TUI task list window.
    TaskStatusChanged(sdk::TaskStatusView),
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
        messages: Vec<TuiChatMessage>,
        session_id: String,
        #[allow(dead_code)]
        created_at: u64,
    },
    /// 会话恢复失败（#636 D2）。TUI 显示错误并退回空 session。
    SessionResumeFailed {
        kind: sdk::SessionResumeFailureKind,
        id: String,
        message: String,
    },
}
pub type UiEvent = AppEvent;

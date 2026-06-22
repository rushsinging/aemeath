//! Chat 事件流类型：事件 / 上下文 / 工具调用状态。

use crate::chat::AskUserQuestionItem;
use crate::chat_result::{ChatResult, ToolResultImage};
use crate::chat_view::{AgentProgressEventView, HookEventView, WorkspaceContextView};
use crate::ChatMessage;
use serde::{Deserialize, Serialize};

/// Runtime stream context used to bind UI events to the authoritative chat/turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEventContext {
    pub chat_id: crate::ids::ChatId,
    pub turn_id: crate::ids::ChatTurnId,
}

impl ChatEventContext {
    pub fn new(chat_id: crate::ids::ChatId, turn_id: crate::ids::ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

/// 工具调用的中间状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatusView {
    PendingArgs,
    Ready,
    Running,
}

/// 已归宿（落账）的单条用户输入，携带 InputId 以供 TUI 按 id 清占位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedInput {
    pub id: crate::InputId,
    pub text: String,
}

/// Chat 事件流中的单个事件。
#[derive(Debug)]
pub enum ChatEvent {
    /// LLM 返回的文本 token。
    Token {
        context: ChatEventContext,
        text: String,
    },
    /// LLM reasoning / thinking token。
    Thinking {
        context: ChatEventContext,
        text: String,
    },
    /// 块完成。
    BlockComplete {
        context: ChatEventContext,
        text: String,
    },
    /// 工具调用开始。
    ToolCallStart {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    /// 工具调用属性/状态更新。
    ToolCallUpdate {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: ToolCallStatusView,
    },
    /// 工具执行结果。
    ToolResult {
        context: ChatEventContext,
        id: crate::ids::ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<ToolResultImage>,
    },
    /// 系统消息。
    SystemMessage(String),
    /// Chat 出错。
    Error(String),
    /// 用量统计。
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    /// runtime 同步当前 messages。
    MessagesSync(Vec<ChatMessage>),
    /// 批量用户输入归宿通知（每条含 InputId）。TUI 用 id 清占位并回显；A2 仅建立通道，消费留待 A3。
    UserMessagesAdded {
        items: Vec<AddedInput>,
    },
    /// Chat 完成。
    Done {
        context: ChatEventContext,
    },
    /// Chat 完成并附带耗时毫秒。
    DoneWithDurationMs {
        context: ChatEventContext,
        duration_ms: u64,
    },
    /// Chat 被取消。
    Cancelled {
        context: ChatEventContext,
    },
    /// 实时 TPS。
    LiveTps(f64),
    /// 当前 turn 变化。
    TurnChanged(usize),
    /// 记录当前 turn 变化的端口事件。
    CurrentTurnChanged(usize),
    /// Hook 事件。
    HookEvent(HookEventView),
    /// AskUserQuestion 批量请求（一次携带多个问题）。
    AskUserBatch {
        items: Vec<AskUserQuestionItem>,
        /// 回传每个问题的答案（顺序与 items 一致）。
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    /// Agent progress 事件投影。
    AgentProgress {
        context: ChatEventContext,
        tool_id: crate::ids::ToolCallId,
        event: AgentProgressEventView,
    },
    /// 工作目录变化。
    WorkingDirectoryChanged {
        path_base: String,
        workspace_root: String,
        workspace: WorkspaceContextView,
    },
    /// 任务列表状态发生变化，TUI 应重新拉取 task_status 快照。
    TasksChanged,
    /// 配置/指令/guidance 文件变更通知。
    ConfigReloaded {
        changed_keys: Vec<String>,
    },
    /// loop 完成 reset 清理后发出，TUI 据此同步清空镜像。
    /// Reasoning Graph 阶段变化（Phase 2）。
    GraphPhaseChanged {
        node: String,
        effort: String,
        prev: String,
    },
    SessionReset,
    /// 批量撤回 pending 输入（#391 S3）。texts 为被撤回文本，TUI join("\n") 还原输入框。
    UserMessagesWithdrawn {
        texts: Vec<String>,
    },
    /// 兼容旧 ChatInput 流结果。
    Result(ChatResult),
}

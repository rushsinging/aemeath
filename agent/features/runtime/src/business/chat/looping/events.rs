use crate::business::session::PersistedWorkspaceContext;
use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
use share::message::Message;
use share::tool::{AgentProgressEvent, ImageData};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTurnContext {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

impl RuntimeTurnContext {
    pub fn new(chat_id: ChatId, turn_id: ChatTurnId) -> Self {
        Self { chat_id, turn_id }
    }
}

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHookEventStatus {
    Running,
    Succeeded,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookExecutionResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub decision: Option<String>,
    pub reason: Option<String>,
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHookEvent {
    pub hook_name: String,
    pub status: RuntimeHookEventStatus,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub result: Option<RuntimeHookExecutionResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeToolCallStatus {
    PendingArgs,
    Ready,
    Running,
}

#[derive(Debug)]
pub enum RuntimeStreamEvent {
    Text {
        context: RuntimeTurnContext,
        text: String,
    },
    Thinking {
        context: RuntimeTurnContext,
        text: String,
    },
    BlockComplete {
        context: RuntimeTurnContext,
        text: String,
    },
    ToolCallStart {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolCallUpdate {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments_delta: Option<String>,
        arguments: Option<serde_json::Value>,
        status: RuntimeToolCallStatus,
    },
    ToolResult {
        context: RuntimeTurnContext,
        id: ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        images: Vec<ImageData>,
    },
    SystemMessage(String),
    Error(String),
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    MessagesSync(Vec<Message>),
    /// 批量用户输入归宿通知（每条含 InputId）。A2 仅建立通道，emit 由 Task 4 完成。
    UserMessagesAdded {
        items: Vec<sdk::AddedInput>,
    },
    /// loop 执行 reset 清理（messages + pending）后发出，通知 TUI 同步清镜像。
    SessionReset,
    /// 批量撤回 pending 输入：texts 为被撤回的 UserMessage 文本（#391 S3）。
    /// TUI 收到后清全部占位 + texts.join("\n") 还原输入框。
    UserMessagesWithdrawn {
        texts: Vec<String>,
    },
    Done {
        context: RuntimeTurnContext,
    },
    DoneWithDuration {
        context: RuntimeTurnContext,
        duration: std::time::Duration,
    },
    Cancelled {
        context: RuntimeTurnContext,
    },
    LiveTps(f64),
    TurnChanged(usize),
    HookEvent(RuntimeHookEvent),
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
    AgentProgress {
        context: RuntimeTurnContext,
        tool_id: ToolCallId,
        event: AgentProgressEvent,
    },
    WorkingDirectoryChanged {
        path_base: String,
        workspace_root: String,
        workspace: PersistedWorkspaceContext,
    },
    TasksChanged,
    /// 配置/指令/guidance 文件变更通知。
    ConfigReloaded {
        changed_keys: Vec<String>,
    },
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;

    fn try_send_event(&self, event: RuntimeStreamEvent);
}

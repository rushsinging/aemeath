use crate::business::reasoning_graph::ReasoningNode;
use crate::business::session::PersistedWorkspaceContext;
use provider::api::ReasoningLevel;
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

/// Compact 进度阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactStage {
    /// 正在分析/切分消息窗口
    Preparing,
    /// 正在执行 LLM 摘要（单次或 map-reduce 的某个 chunk）
    Summarizing,
    /// 正在清理 tool pairs / 组装结果
    Finalizing,
}

impl CompactStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Summarizing => "summarizing",
            Self::Finalizing => "finalizing",
        }
    }
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
    /// 批量用户输入归宿通知（每条含 InputId + 派生 Message，用于 #507 修复后 TUI 回显含占位符）。
    /// A2 仅建立通道，emit 由 Task 4 完成；#507 修复后 payload 改为 (InputId, Message) 元组。
    UserMessagesAdded {
        items: Vec<(sdk::InputId, Message)>,
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
    /// Reasoning Graph 阶段变化通知（Phase 2）。
    GraphPhaseChanged {
        node: ReasoningNode,
        effort: ReasoningLevel,
        prev: ReasoningNode,
    },
    /// Compact 进度通知。`current`/`total` 为 map-reduce chunk 计数（单次摘要时为 None）。
    CompactProgress {
        stage: CompactStage,
        current: Option<usize>,
        total: Option<usize>,
    },
    /// 模型切换完成通知（#497）。runtime idle 分支执行 switch_model 后回传结果。
    ModelSwitched {
        result: sdk::ModelSwitchResult,
    },
    /// Reasoning 模式切换完成通知（#497）。runtime idle 分支执行 set_thinking 后回传结果。
    ThinkingChanged {
        enabled: bool,
    },
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;

    fn try_send_event(&self, event: RuntimeStreamEvent);
}

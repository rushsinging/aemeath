use crate::business::session::PersistedWorkspaceContext;
use share::message::Message;
use share::tool::{AgentProgressEvent, ImageData};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTurnContext {
    pub chat_id: String,
    pub turn_id: String,
}

impl RuntimeTurnContext {
    pub fn new(chat_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            chat_id: chat_id.into(),
            turn_id: turn_id.into(),
        }
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
    TextBlockComplete {
        context: RuntimeTurnContext,
        text: String,
    },
    ToolCallStart {
        context: RuntimeTurnContext,
        id: String,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ToolArgumentsDelta {
        context: RuntimeTurnContext,
        id: String,
        provider_id: Option<String>,
        index: usize,
        name: String,
        partial_args: String,
    },
    ToolCall {
        context: RuntimeTurnContext,
        id: String,
        provider_id: String,
        name: String,
        index: Option<usize>,
        summary: String,
    },
    ToolResult {
        context: RuntimeTurnContext,
        id: String,
        provider_id: String,
        tool_name: String,
        output: String,
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
    Done,
    DoneWithDuration(std::time::Duration),
    Cancelled,
    LiveTps(f64),
    TurnChanged(usize),
    HookEvent(RuntimeHookEvent),
    AskUser {
        id: String,
        question: String,
        options: Vec<sdk::OptionItem>,
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    AgentProgress {
        tool_id: String,
        event: AgentProgressEvent,
    },
    WorkingDirectoryChanged {
        path_base: String,
        working_root: String,
        workspace: PersistedWorkspaceContext,
    },
    TasksChanged,
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;

    fn try_send_event(&self, event: RuntimeStreamEvent);
}

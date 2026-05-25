use crate::api::core::message::Message;
use crate::api::core::session::WorkspaceContext;
use crate::api::core::tool::{AgentProgressEvent, ImageData};
use std::future::Future;
use std::pin::Pin;

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[derive(Debug)]
pub enum RuntimeStreamEvent {
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
        summary: String,
    },
    ToolResult {
        id: String,
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
    StopFailureHook {
        system_message: Option<String>,
        additional_context: Option<String>,
    },
    AskUser {
        id: String,
        question: String,
        options: Vec<String>,
        allow_free_input: bool,
        multi_select: bool,
        default: Option<String>,
        reply_tx: tokio::sync::oneshot::Sender<String>,
    },
    AgentProgress {
        tool_id: String,
        event: AgentProgressEvent,
    },
    HookStart {
        event: String,
        command: String,
    },
    HookEnd {
        event: String,
        blocked: bool,
        error: Option<String>,
    },
    WorkingDirectoryChanged {
        path_base: String,
        working_root: String,
        workspace: WorkspaceContext,
    },
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;

    fn try_send_event(&self, event: RuntimeStreamEvent);
}

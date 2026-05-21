use aemeath_core::message::Message;
use aemeath_core::tool::{AgentProgressEvent, ImageData};

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
        summary: String,
    },
    ToolResult {
        id: String,
        tool_name: String,
        output: String,
        is_error: bool,
        images: Vec<ImageData>,
    },
    Usage {
        input: u32,
        output: u32,
        last_input: u32,
        elapsed_secs: f64,
    },
    Error(String),
    Cancelled,
    MessagesSync(Vec<Message>),
    Done,
    DoneWithDuration(std::time::Duration),
    LiveTps(f64),
    ClipboardImage(crate::image::ProcessedImage),
    SystemMessage(String),
    ReflectionStarted,
    ReflectionUsage {
        input: u32,
        output: u32,
    },
    ReflectionDone {
        output: aemeath_core::reflection::ReflectionOutput,
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
        event: AgentProgressEvent,
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
}

pub type UiEvent = AppEvent;

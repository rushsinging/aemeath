use super::{StatusContextUpdate, UiEvent};
use ::runtime::api::chat::{
    ChatEventSink, EventFuture, QueueDrainPort, QueueFuture, RuntimeStreamEvent,
};
use ::runtime::api::core::tool::ToolRegistry;
use ::runtime::api::provider::types::SystemBlock;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct TuiEventSink {
    tx: mpsc::Sender<UiEvent>,
}

impl TuiEventSink {
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }
}

impl ChatEventSink for TuiEventSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            let ui_event = runtime_event_to_ui_event(event);
            let _ = self.tx.send(ui_event).await;
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        let ui_event = runtime_event_to_ui_event(event);
        if let Err(e) = self.tx.try_send(ui_event) {
            log::warn!("UI channel full, dropped runtime stream event: {e}");
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TuiQueueDrainPort {
    tx: mpsc::Sender<UiEvent>,
}

impl TuiQueueDrainPort {
    #[allow(dead_code)]
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }
}

impl QueueDrainPort for TuiQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
        Box::pin(async move {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            if self
                .tx
                .send(UiEvent::DrainQueuedInput { reply_tx })
                .await
                .is_err()
            {
                return None;
            }
            match reply_rx.await {
                Ok(queued) if !queued.is_empty() => Some(queued),
                _ => None,
            }
        })
    }
}

fn runtime_event_to_ui_event(event: RuntimeStreamEvent) -> UiEvent {
    match event {
        RuntimeStreamEvent::Text(text) => UiEvent::Text(text),
        RuntimeStreamEvent::Thinking(text) => UiEvent::Thinking(text),
        RuntimeStreamEvent::TextBlockComplete(text) => UiEvent::TextBlockComplete(text),
        RuntimeStreamEvent::ToolCallStart { name, index } => UiEvent::ToolCallStart { name, index },
        RuntimeStreamEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        } => UiEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        },
        RuntimeStreamEvent::ToolCall { id, name, summary } => {
            UiEvent::ToolCall { id, name, summary }
        }
        RuntimeStreamEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        } => UiEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        },
        RuntimeStreamEvent::SystemMessage(msg) => UiEvent::SystemMessage(msg),
        RuntimeStreamEvent::Error(msg) => UiEvent::Error(msg),
        RuntimeStreamEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        RuntimeStreamEvent::MessagesSync(messages) => UiEvent::MessagesSync(messages),
        RuntimeStreamEvent::Done => UiEvent::Done,
        RuntimeStreamEvent::DoneWithDuration(duration) => UiEvent::DoneWithDuration(duration),
        RuntimeStreamEvent::Cancelled => UiEvent::Cancelled,
        RuntimeStreamEvent::LiveTps(tps) => UiEvent::LiveTps(tps),
        RuntimeStreamEvent::TurnChanged(turn) => {
            ::runtime::api::bootstrap::set_current_turn(turn);
            UiEvent::SystemMessage(String::new())
        }
        RuntimeStreamEvent::StopFailureHook {
            system_message,
            additional_context,
        } => UiEvent::StopFailureHook {
            system_message,
            additional_context,
        },
        RuntimeStreamEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        } => UiEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        },
        RuntimeStreamEvent::AgentProgress { tool_id, event } => {
            UiEvent::AgentProgress { tool_id, event }
        }
        RuntimeStreamEvent::HookStart { event, command } => UiEvent::HookStart { event, command },
        RuntimeStreamEvent::HookEnd {
            event,
            blocked,
            error,
        } => UiEvent::HookEnd {
            event,
            blocked,
            error,
        },
        RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::app::display_status_path(std::path::Path::new(&path_base)),
            working_root: crate::tui::app::display_status_path(std::path::Path::new(&working_root)),
            branch: crate::tui::app::git_branch_for(std::path::Path::new(&working_root)),
            kind: crate::tui::app::worktree_kind_for(std::path::Path::new(&working_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_working_root: std::path::PathBuf::from(working_root),
            workspace,
        }),
    }
}

/// Owned context needed to spawn a background processing task.
pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub queue_request_tx: mpsc::Sender<UiEvent>,
    pub client: Arc<::runtime::api::provider::client::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<::runtime::api::core::message::Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub workspace_context: Option<::runtime::api::session::WorkspaceContext>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<::runtime::api::core::tool::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<::runtime::api::core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: ::runtime::api::hook::hook::HookRunner,
    pub memory_config: ::runtime::api::core::config::MemoryConfig,
    pub json_logger: Option<Arc<std::sync::Mutex<::runtime::api::storage::logging::JsonLogger>>>,
}

/// Borrowed references to the shared state needed for spawning.
/// Used in the processing pipeline to avoid passing many individual parameters.
pub(crate) struct SpawnContextRefs<'a> {
    pub client: &'a Arc<::runtime::api::provider::client::LlmClient>,
    pub registry: &'a Arc<ToolRegistry>,
    pub system_blocks: &'a Vec<SystemBlock>,
    pub system_prompt_text: &'a str,
    pub user_context: &'a str,
    pub context_size: usize,
    pub read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders:
        &'a Arc<std::sync::Mutex<::runtime::api::core::tool::SessionReminders>>,
    pub agent_runner: &'a Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: &'a Arc<AtomicBool>,
    pub task_store: &'a Arc<::runtime::api::core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub hook_runner: &'a ::runtime::api::hook::hook::HookRunner,
    pub memory_config: &'a ::runtime::api::core::config::MemoryConfig,
    pub json_logger:
        &'a Option<Arc<std::sync::Mutex<::runtime::api::storage::logging::JsonLogger>>>,
}

/// Spawn the background LLM processing task.
pub(super) fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
        let sink = TuiEventSink::new(ctx.tx);
        let queue = TuiQueueDrainPort::new(ctx.queue_request_tx);
        ::runtime::api::chat::process_chat_loop(::runtime::api::chat::ChatLoopContext {
            sink,
            queue,
            client: ctx.client,
            registry: ctx.registry,
            system_blocks: ctx.system_blocks,
            system_prompt_text: ctx.system_prompt_text,
            user_context: ctx.user_context,
            messages: ctx.messages,
            context_size: ctx.context_size,
            cwd: ctx.cwd,
            workspace_context: ctx.workspace_context,
            session_id: ctx.session_id,
            read_files: ctx.read_files,
            session_reminders: ctx.session_reminders,
            agent_runner: ctx.agent_runner,
            allow_all: ctx.allow_all,
            interrupted: ctx.interrupted,
            cancel: ctx.cancel,
            task_store: ctx.task_store,
            max_tool_concurrency: ctx.max_tool_concurrency,
            max_agent_concurrency: ctx.max_agent_concurrency,
            agent_semaphore: ctx.agent_semaphore,
            hook_runner: ctx.hook_runner,
            memory_config: ctx.memory_config,
            json_logger: ctx.json_logger,
        })
        .await;
    });
}

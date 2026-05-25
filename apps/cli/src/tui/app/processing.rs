use super::UiEvent;
use kernel::tool::ToolRegistry;
use provider::types::SystemBlock;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Owned context needed to spawn a background processing task.
pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub queue_request_tx: mpsc::Sender<UiEvent>,
    pub client: Arc<provider::client::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<kernel::message::Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub workspace_context: Option<kernel::session::WorkspaceContext>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<kernel::memory::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn kernel::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<kernel::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: kernel::hook::HookRunner,
    pub memory_config: kernel::config::MemoryConfig,
    pub json_logger: Option<Arc<std::sync::Mutex<kernel::logging::JsonLogger>>>,
}

/// Borrowed references to the shared state needed for spawning.
/// Used in the processing pipeline to avoid passing many individual parameters.
pub(crate) struct SpawnContextRefs<'a> {
    pub client: &'a Arc<provider::client::LlmClient>,
    pub registry: &'a Arc<ToolRegistry>,
    pub system_blocks: &'a Vec<SystemBlock>,
    pub system_prompt_text: &'a str,
    pub user_context: &'a str,
    pub context_size: usize,
    pub read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: &'a Arc<std::sync::Mutex<kernel::memory::SessionReminders>>,
    pub agent_runner: &'a Option<Arc<dyn kernel::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: &'a Arc<AtomicBool>,
    pub task_store: &'a Arc<kernel::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub hook_runner: &'a kernel::hook::HookRunner,
    pub memory_config: &'a kernel::config::MemoryConfig,
    pub json_logger: &'a Option<Arc<std::sync::Mutex<kernel::logging::JsonLogger>>>,
}

/// Spawn the background LLM processing task.
pub(super) fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
        super::stream::process_in_background(
            ctx.tx,
            ctx.queue_request_tx,
            ctx.client,
            ctx.registry,
            ctx.system_blocks,
            ctx.system_prompt_text,
            ctx.user_context,
            ctx.messages,
            ctx.context_size,
            ctx.cwd,
            ctx.workspace_context,
            ctx.session_id,
            ctx.read_files,
            ctx.session_reminders,
            ctx.agent_runner,
            ctx.allow_all,
            ctx.interrupted,
            ctx.cancel,
            ctx.task_store,
            ctx.max_tool_concurrency,
            ctx.max_agent_concurrency,
            ctx.agent_semaphore,
            ctx.hook_runner,
            ctx.memory_config,
            ctx.json_logger,
        )
        .await;
    });
}

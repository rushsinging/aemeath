use super::UiEvent;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::types::SystemBlock;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Owned context needed to spawn a background processing task.
pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub client: Arc<aemeath_llm::client::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<aemeath_core::message::Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<aemeath_core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: aemeath_core::hook::HookRunner,
}

/// Borrowed references to the shared state needed for spawning.
/// Used in event_handler to avoid passing many individual parameters.
pub(crate) struct SpawnContextRefs<'a> {
    pub client: &'a Arc<aemeath_llm::client::LlmClient>,
    pub registry: &'a Arc<ToolRegistry>,
    pub system_blocks: &'a Vec<SystemBlock>,
    pub system_prompt_text: &'a str,
    pub user_context: &'a str,
    pub context_size: usize,
    pub read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub agent_runner: &'a Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: &'a Arc<AtomicBool>,
    pub task_store: &'a Arc<aemeath_core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub hook_runner: &'a aemeath_core::hook::HookRunner,
}

/// Spawn the background LLM processing task.
pub(super) fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
        super::stream::process_in_background(
            ctx.tx, ctx.client, ctx.registry, ctx.system_blocks,
            ctx.system_prompt_text, ctx.user_context, ctx.messages,
            ctx.context_size, ctx.cwd, ctx.session_id, ctx.read_files,
            ctx.agent_runner, ctx.allow_all, ctx.interrupted, ctx.cancel,
            ctx.task_store,
            ctx.max_tool_concurrency, ctx.max_agent_concurrency, ctx.agent_semaphore,
            ctx.hook_runner,
        ).await;
    });
}

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use tools::api::ToolContext;

pub(crate) struct ToolContextParts {
    pub(crate) cwd: PathBuf,
    pub(crate) working_root: Arc<Mutex<PathBuf>>,
    pub(crate) path_base: Arc<Mutex<PathBuf>>,
    pub(crate) cancel: CancellationToken,
    pub(crate) read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) agent_runner: Option<Arc<dyn tools::api::AgentRunner>>,
    pub(crate) session_reminders: Arc<Mutex<share::tool::SessionReminders>>,
    pub(crate) memory_config: share::config::MemoryConfig,
    pub(crate) allow_all: bool,
    pub(crate) max_tool_concurrency: usize,
    pub(crate) max_agent_concurrency: usize,
    pub(crate) agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub(crate) session_id: String,
    pub(crate) context_stack: Arc<Mutex<Vec<share::tool::WorkingContext>>>,
}

pub(crate) fn build_tool_context(parts: ToolContextParts) -> ToolContext {
    ToolContext {
        cwd: parts.cwd,
        working_root: parts.working_root,
        path_base: parts.path_base,
        cancel: parts.cancel,
        read_files: parts.read_files,
        agent_runner: parts.agent_runner,
        session_reminders: Some(parts.session_reminders),
        memory_config: parts.memory_config,
        plan_mode: None,
        allow_all: parts.allow_all,
        max_tool_concurrency: parts.max_tool_concurrency,
        max_agent_concurrency: parts.max_agent_concurrency,
        agent_semaphore: parts.agent_semaphore,
        progress_tx: None,
        parent_session_id: Some(parts.session_id),
        context_stack: parts.context_stack,
    }
}

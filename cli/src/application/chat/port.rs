use super::request::ChatLaunchRequest;
use aemeath_core::config::MemoryConfig;
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::JsonLogger;
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct NoTuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
}

pub(crate) struct TuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiChatOutcome {
    pub session_id: String,
}

#[async_trait(?Send)]
pub(crate) trait ChatRuntimePort {
    async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String>;

    async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String>;
}

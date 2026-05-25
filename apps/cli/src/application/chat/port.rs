use super::request::{NoTuiChatLaunch, TuiChatLaunch};
use ::runtime::api::core::config::MemoryConfig;
use ::runtime::api::core::hook::HookRunner;
use ::runtime::api::core::logging::JsonLogger;
use ::runtime::api::core::skill::Skill;
use ::runtime::api::core::task::TaskStore;
use ::runtime::api::core::tool::{AgentRunner, ToolRegistry};
use ::runtime::api::provider::client::LlmClient;
use ::runtime::api::provider::types::SystemBlock;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct ChatRuntimeContext {
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
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String>;

    async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String>;
}

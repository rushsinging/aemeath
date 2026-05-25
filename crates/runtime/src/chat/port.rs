use super::request::{NoTuiChatLaunch, TuiChatLaunch};
use crate::api::core::config::MemoryConfig;
use crate::api::core::hook::HookRunner;
use crate::api::core::skill::Skill;
use crate::api::core::task::TaskStore;
use crate::api::core::tool::{AgentRunner, ToolRegistry};
use crate::api::provider::client::LlmClient;
use crate::api::provider::types::SystemBlock;
use crate::api::storage::logging::JsonLogger;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct ChatRuntimeContext {
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
pub struct TuiChatOutcome {
    pub session_id: String,
}

#[async_trait(?Send)]
pub trait ChatRuntimePort {
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

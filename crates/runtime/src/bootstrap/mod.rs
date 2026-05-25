pub mod concurrency;
pub mod model_runtime;
pub mod permissions;
pub mod provider_client;
pub mod runtime_support;

use std::path::PathBuf;

pub use concurrency::resolve_concurrency_limits;
pub use model_runtime::{
    resolve_model_runtime_settings, ModelRuntimeSettings, ReasoningConfigInput,
};
pub use permissions::apply_config_permission_mode;
pub use provider_client::{build_llm_client, resolve_api_key, resolve_base_url};
pub use runtime_support::{
    build_agent_runner, build_hook_runner, build_json_logger, start_session,
};

use crate::api::core::config::models::ResolvedModel;
use crate::api::core::mcp_manager::McpConnectionManager;
use crate::chat::ChatRuntimeContext;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatModeSelection {
    NoTui,
    Tui,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatBootstrapArgs {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_tokens: Option<u32>,
    pub verbose: bool,
    pub no_markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub tui: bool,
    pub no_tui: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
    pub reasoning_effort: Option<String>,
}

impl ChatBootstrapArgs {
    pub fn mode_selection(&self) -> ChatModeSelection {
        if self.no_tui || !self.tui {
            ChatModeSelection::NoTui
        } else {
            ChatModeSelection::Tui
        }
    }
}

pub struct ChatBootstrap {
    pub args: ChatBootstrapArgs,
    pub cwd: PathBuf,
    pub resolved_model: ResolvedModel,
    pub session_id: String,
    pub context: ChatRuntimeContext,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub mode_selection: ChatModeSelection,
    pub _mcp_manager: Arc<McpConnectionManager>,
}

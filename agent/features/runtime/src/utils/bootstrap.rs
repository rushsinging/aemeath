pub mod claude_settings_adapter;
pub mod concurrency;
pub mod config_manager;
pub mod config_patch;
pub mod config_paths;
pub mod logging_setup;
pub mod mcp_loader;
pub mod model_runtime;
pub mod permissions;
pub mod provider_client;
pub mod runtime_support;

use crate::core::port::ChatRuntimeContext;
pub use concurrency::resolve_concurrency_limits;
pub use logging_setup::{init_logging, set_current_turn, set_session_id};
pub use mcp_loader::{load_mcp_manager, parse_mcp_servers_config, spawn_mcp_connect};
pub use model_runtime::{resolve_model_runtime_settings, ModelRuntimeSettings};
pub use permissions::apply_config_permission_mode;
pub use provider_client::{build_llm_client, resolve_api_key, resolve_base_url};
pub use runtime_support::{build_agent_runner, build_hook_runner, start_session};
use share::config::models::ResolvedModel;
use share::config::Config;
use std::sync::Arc;
use tools::api::McpConnectionManager;

/// 合并 context_size（优先级：CLI > env > config model.context_size > resolved model contextWindow > 默认 128000）。
pub fn resolve_context_size(
    cli_context_size: usize,
    config_file: Option<&Config>,
    model_context_window: usize,
) -> usize {
    let config_ctx = config_file.map(|c| c.model.context_size).unwrap_or(0);
    let env_val = std::env::var("AEMEATH_CONTEXT_SIZE").ok();
    log::debug!(
        target: "aemeath:agent:runtime",
        "resolve_context_size: cli={}, model_context_window={}, config.model.context_size={}, env AEMEATH_CONTEXT_SIZE={:?}",
        cli_context_size, model_context_window, config_ctx, env_val,
    );
    // CLI 非零 → 用户显式设置了
    if cli_context_size > 0 {
        log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=cli, result={}", cli_context_size);
        return cli_context_size;
    }
    // env AEMEATH_CONTEXT_SIZE
    if let Ok(env_val) = std::env::var("AEMEATH_CONTEXT_SIZE") {
        if let Ok(parsed) = env_val.parse::<usize>() {
            if parsed > 0 {
                log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=env, result={}", parsed);
                return parsed;
            }
        }
    }
    // config model.context_size（全局覆盖）
    if let Some(config) = config_file {
        if config.model.context_size > 0 {
            log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=config_model, result={}", config.model.context_size);
            return config.model.context_size;
        }
    }
    // resolved provider model 的 contextWindow（来自 models.providers.*.models[].contextWindow）
    if model_context_window > 0 {
        log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=provider_model, result={}", model_context_window);
        return model_context_window;
    }
    log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=default, result=128000");
    128_000
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LegacyChatBootstrapArgs {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<std::path::PathBuf>,
    pub max_tokens: Option<u32>,
    pub verbose: bool,
    pub no_markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
    pub max_reasoning: Option<String>,
}

pub type ChatBootstrapArgs = sdk::ChatBootstrapArgs;

pub struct InstructionsLoadedHookRunner<'a> {
    pub hook_runner: &'a hook::api::HookRunner,
    pub workspace_root: &'a std::path::Path,
}

#[async_trait::async_trait(?Send)]
impl prompt::api::guidance::InstructionsLoadedHook for InstructionsLoadedHookRunner<'_> {
    async fn on_instructions_loaded(&self, file_path: &str, instruction_type: &str) {
        let _ = self
            .hook_runner
            .on_instructions_loaded(file_path, instruction_type, self.workspace_root)
            .await;
    }
}

pub struct ChatBootstrap {
    pub args: ChatBootstrapArgs,
    pub cwd: std::path::PathBuf,
    pub resolved_model: ResolvedModel,
    pub session_id: String,
    pub context: ChatRuntimeContext,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub _mcp_manager: Arc<McpConnectionManager>,
}

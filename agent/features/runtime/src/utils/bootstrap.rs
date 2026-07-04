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
use std::sync::Arc;
use tools::api::McpConnectionManager;

/// 合并 context_size（优先级：CLI > snapshot(env+config 已合并) > resolved model contextWindow > 默认 128000）。
///
/// env 读取已由 `EnvAdapter` 在 snapshot 构建时完成，此处不再直接读 env。
pub fn resolve_context_size(
    cli_context_size: usize,
    snapshot_context_size: usize,
    model_context_window: usize,
) -> usize {
    log::debug!(
        target: "aemeath:agent:runtime",
        "resolve_context_size: cli={}, snapshot={}, model_context_window={}",
        cli_context_size, snapshot_context_size, model_context_window,
    );
    // CLI 非零 → 用户显式设置了
    if cli_context_size > 0 {
        log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=cli, result={}", cli_context_size);
        return cli_context_size;
    }
    // snapshot 值（env > file 已在 ConfigAppService::load 中合并）
    if snapshot_context_size > 0 {
        log::debug!(target: "aemeath:agent:runtime", "resolve_context_size branch=snapshot, result={}", snapshot_context_size);
        return snapshot_context_size;
    }
    // resolved provider model 的 contextWindow
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

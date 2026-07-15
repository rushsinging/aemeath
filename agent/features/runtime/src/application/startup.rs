pub mod claude_settings_adapter;
pub mod concurrency;
pub mod config_paths;
pub mod logging_setup;
pub mod mcp_loader;
pub mod model_runtime;
pub mod permissions;
pub mod provider_client;
pub mod runtime_support;

pub use concurrency::resolve_concurrency_limits;
pub use logging_setup::{init_logging, set_current_turn, set_session_id};
pub use mcp_loader::spawn_mcp_connect;
pub use model_runtime::{resolve_model_runtime_settings, ModelRuntimeSettings};
pub use permissions::apply_config_permission_mode;
pub use provider_client::{build_llm_client, resolve_api_key, resolve_base_url};
pub use runtime_support::{build_agent_runner, build_hook_runner, start_session};

pub type ChatBootstrapArgs = sdk::ChatBootstrapArgs;

pub struct InstructionsLoadedHookRunner<'a> {
    pub hook_runner: &'a hook::api::HookRunner,
    pub workspace_root: &'a std::path::Path,
}

#[async_trait::async_trait(?Send)]
impl context::guidance::InstructionsLoadedHook for InstructionsLoadedHookRunner<'_> {
    async fn on_instructions_loaded(&self, file_path: &str, instruction_type: &str) {
        let _ = self
            .hook_runner
            .on_instructions_loaded(file_path, instruction_type, self.workspace_root)
            .await;
    }
}

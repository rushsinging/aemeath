pub mod claude_settings_adapter;
pub mod concurrency;
pub mod config_paths;
pub mod model_runtime;
pub mod runtime_support;

use hook::HookDispatchContext;
use std::path::PathBuf;
use std::sync::Arc;

pub use concurrency::resolve_concurrency_limits;
pub use model_runtime::{resolve_model_runtime_settings, ModelRuntimeSettings};
pub use runtime_support::{build_agent_runner, start_session};

pub type ChatBootstrapArgs = sdk::ChatBootstrapArgs;

pub struct InstructionsLoadedHook {
    pub hooks: Arc<dyn hook::HookPort>,
    pub workspace_root: PathBuf,
}

#[async_trait::async_trait(?Send)]
impl context::guidance::InstructionsLoadedHook for InstructionsLoadedHook {
    async fn on_instructions_loaded(&self, file_path: &str, instruction_type: &str) {
        let _ = self
            .hooks
            .dispatch_at(
                hook::HookInvocation::InstructionsLoaded(hook::InstructionsInput {
                    file_path: file_path.to_string(),
                    instruction_type: instruction_type.to_string(),
                }),
                HookDispatchContext::new(&self.workspace_root),
                &tokio_util::sync::CancellationToken::new(),
            )
            .await;
    }
}

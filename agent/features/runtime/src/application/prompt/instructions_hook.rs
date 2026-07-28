use hook::HookDispatchContext;
use std::path::PathBuf;
use std::sync::Arc;

pub struct PromptInstructionsHook {
    pub hooks: Arc<dyn hook::HookPort>,
    pub workspace_root: PathBuf,
}

#[async_trait::async_trait(?Send)]
impl context::guidance::InstructionsLoadedHook for PromptInstructionsHook {
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

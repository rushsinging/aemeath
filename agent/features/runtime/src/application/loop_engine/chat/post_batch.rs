use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::hook_ui::dispatch_hook;
use hook::{HookInvocation, HookPort, PostToolBatchInput};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_post_tool_batch(
    hook_port: &Arc<dyn HookPort>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    cancel: &CancellationToken,
    tool_count: usize,
    step_count: usize,
    workspace_read: &Arc<dyn project::WorkspaceRead>,
) {
    let workspace_root = workspace_read.current_workspace_root();
    let _ = dispatch_hook(
        hook_port,
        activities,
        step_id,
        HookInvocation::PostToolBatch(PostToolBatchInput {
            tool_count,
            summary: format!("batch with {tool_count} tools after {step_count} run steps"),
        }),
        &workspace_root,
        cancel,
    )
    .await;
}

#[cfg(test)]
#[path = "post_batch_tests.rs"]
mod tests;

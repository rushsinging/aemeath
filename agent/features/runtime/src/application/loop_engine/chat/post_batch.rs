use crate::application::activity::ActivityCoordinator;
use crate::application::loop_engine::chat::hook_ui::dispatch_hook;
use hook::{HookDispatcher, HookInvocationData};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_post_tool_batch(
    hook_port: &Arc<dyn HookDispatcher>,
    activities: &ActivityCoordinator,
    step_id: &sdk::RunStepId,
    session_id: &str,
    cancel: &CancellationToken,
    tool_count: usize,
    step_count: usize,
    workspace_read: &Arc<dyn project::WorkspaceReader>,
) {
    let workspace_root = workspace_read.current_workspace_root();
    let _ = dispatch_hook(
        hook_port,
        activities,
        step_id,
        HookInvocationData::PostToolBatch {
            tool_count,
            summary: format!("batch with {tool_count} tools after {step_count} run steps"),
        },
        &workspace_root,
        session_id,
        cancel,
    )
    .await;
}

#[cfg(test)]
#[path = "post_batch_tests.rs"]
mod tests;

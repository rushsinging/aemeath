use crate::application::chat::looping::hook_ui::dispatch_hook;
use crate::application::chat::looping::ChatEventSink;
use hook::{HookInvocation, HookPort, PostToolBatchInput};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_post_tool_batch<S>(
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    cancel: &CancellationToken,
    tool_count: usize,
    turn_count: usize,
    workspace_root: &std::path::Path,
) where
    S: ChatEventSink,
{
    let _ = dispatch_hook(
        hook_port,
        sink,
        HookInvocation::PostToolBatch(PostToolBatchInput {
            tool_count,
            summary: format!("batch with {tool_count} tools after {turn_count} turns"),
        }),
        workspace_root,
        cancel,
    )
    .await;
}

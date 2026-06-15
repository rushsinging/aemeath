use crate::business::agent::ToolCall;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::tools::{run_post_tool_hooks, send_tool_result, UiToolResult};
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use hook::api::{HookData, ToolHookData};
use share::config::hooks::HookEvent;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::api::{ToolExecutionContext, ToolRegistry};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    context: &RuntimeTurnContext,
    agent_approved: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    agent_ctx: &ToolExecutionContext,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    max_agent_concurrency: usize,
    cancel: &CancellationToken,
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let batch_size = max_agent_concurrency.max(1);
    let mut agent_results = Vec::new();
    for batch in agent_approved.chunks(batch_size) {
        if cancel.is_cancelled() {
            break;
        }
        let agent_futures: Vec<_> = batch
            .iter()
            .map(|call| {
                let call = ToolCall {
                    id: call.id.clone(),
                    provider_id: call.provider_id.clone(),
                    name: call.name.clone(),
                    index: call.index,
                    input: call.input.clone(),
                };
                let sink = sink.clone();
                let hook_ui = hook_ui.clone();
                let mut ag_ctx = agent_ctx.clone();
                let hook_runner = hook_runner.clone();
                let registry_ref = registry.clone();
                let context = context.clone();
                async move {
                    execute_one_agent(
                        &context,
                        call,
                        sink,
                        hook_ui,
                        hook_runner,
                        registry_ref,
                        &mut ag_ctx,
                    )
                    .await
                }
            })
            .collect();
        let batch_results: Vec<Vec<UiToolResult>> = futures::future::join_all(agent_futures).await;
        agent_results.extend(batch_results.into_iter().flatten());
    }
    agent_results
}

async fn execute_one_agent<S>(
    context: &RuntimeTurnContext,
    call: ToolCall,
    sink: S,
    hook_ui: HookUi<S>,
    hook_runner: hook::api::HookRunner,
    registry: Arc<ToolRegistry>,
    ag_ctx: &mut ToolExecutionContext,
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let pre_results = hook_ui
        .run_plain(
            &hook_runner,
            HookEvent::PreToolUse,
            Some(&call.name),
            HookData::Tool(ToolHookData {
                tool_name: call.name.clone(),
                tool_input: call.input.clone(),
                tool_output: None,
                is_error: None,
            }),
        )
        .await;
    if let Some(blocked_result) = pre_results.iter().find(|r| r.blocked) {
        let error_detail = blocked_result
            .error
            .as_deref()
            .unwrap_or("Blocked by PreToolUse hook");
        let result = (
            call.id.clone(),
            call.provider_id.clone(),
            error_detail.to_string(),
            serde_json::json!({ "text": error_detail }),
            true,
            Vec::new(),
        );
        send_tool_result(&sink, context, &call, &result).await;
        return vec![result];
    }

    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<share::tool::AgentProgressEvent>(32);
    ag_ctx.progress_tx = Some(prog_tx);
    let call_id = call.id.clone();
    let ui_sink = sink.clone();
    let progress_context = context.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(event) = prog_rx.recv().await {
            let _ = ui_sink
                .send_event(RuntimeStreamEvent::AgentProgress {
                    context: progress_context.clone(),
                    tool_id: call_id.clone(),
                    event,
                })
                .await;
        }
    });

    let agent_tool = registry
        .get("Agent")
        .expect("Agent tool not found in registry");
    let result = agent_tool.call(call.input.clone(), ag_ctx).await;
    let working_root = ag_ctx.workspace_read().current_root();
    let in_worktree = ag_ctx.workspace_read().in_worktree();
    hook_runner.set_project_context(working_root.display().to_string(), in_worktree);
    let workspace = project::api::WorkspacePersist::snapshot(ag_ctx.workspace.as_ref());
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            working_root: workspace.working_root.clone(),
            workspace,
        })
        .await;
    let results = vec![(
        call.id.clone(),
        call.provider_id.clone(),
        result.output,
        result.content,
        result.is_error,
        result.images,
    )];
    ag_ctx.progress_tx = None;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    for (id, provider_id, output, content, is_error, images) in &results {
        run_post_tool_hooks(&sink, &hook_ui, &hook_runner, &call, output, *is_error).await;
        send_tool_result(
            &sink,
            context,
            &call,
            &(
                id.clone(),
                provider_id.clone(),
                output.clone(),
                content.clone(),
                *is_error,
                images.clone(),
            ),
        )
        .await;
    }
    results
}

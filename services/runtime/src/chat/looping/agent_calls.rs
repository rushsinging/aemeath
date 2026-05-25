use crate::api::core::agent::ToolCall;
use crate::api::core::config::hooks::HookEvent;
use crate::api::hook::hook::{HookData, ToolHookData};
use crate::api::core::tool::ToolRegistry;
use crate::chat::looping::hook_ui::HookUi;
use crate::chat::looping::tools::{run_post_tool_hooks, send_tool_result, UiToolResult};
use crate::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls<S>(
    agent_approved: &[&ToolCall],
    registry: &Arc<ToolRegistry>,
    agent_ctx: &crate::api::core::tool::ToolContext,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &crate::api::hook::hook::HookRunner,
    max_agent_concurrency: usize,
    interrupted: &Arc<AtomicBool>,
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let batch_size = max_agent_concurrency.max(1);
    let mut agent_results = Vec::new();
    for batch in agent_approved.chunks(batch_size) {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }
        let agent_futures: Vec<_> = batch
            .iter()
            .map(|call| {
                let call = ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                };
                let sink = sink.clone();
                let hook_ui = hook_ui.clone();
                let mut ag_ctx = agent_ctx.clone();
                let hook_runner = hook_runner.clone();
                let registry_ref = registry.clone();
                async move {
                    execute_one_agent(call, sink, hook_ui, hook_runner, registry_ref, &mut ag_ctx)
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
    call: ToolCall,
    sink: S,
    hook_ui: HookUi<S>,
    hook_runner: crate::api::hook::hook::HookRunner,
    registry: Arc<ToolRegistry>,
    ag_ctx: &mut crate::api::core::tool::ToolContext,
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
    if pre_results.iter().any(|r| r.blocked) {
        let result = (
            call.id.clone(),
            "Blocked by PreToolUse hook".to_string(),
            true,
            Vec::new(),
        );
        send_tool_result(&sink, &call, &result).await;
        return vec![result];
    }

    let (prog_tx, mut prog_rx) =
        tokio::sync::mpsc::channel::<crate::api::core::tool::AgentProgressEvent>(32);
    ag_ctx.progress_tx = Some(prog_tx);
    let call_id = call.id.clone();
    let ui_sink = sink.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(event) = prog_rx.recv().await {
            let _ = ui_sink
                .send_event(RuntimeStreamEvent::AgentProgress {
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
    let working_root = ag_ctx.current_working_root();
    hook_runner.set_project_dir(working_root.display().to_string());
    let workspace = crate::api::project::worktree::workspace_context_from_tool_context(ag_ctx);
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            working_root: workspace.working_root.clone(),
            workspace,
        })
        .await;
    let results = vec![(
        call.id.clone(),
        result.output,
        result.is_error,
        result.images,
    )];
    ag_ctx.progress_tx = None;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    for (id, output, is_error, images) in &results {
        run_post_tool_hooks(&sink, &hook_ui, &hook_runner, &call, output, *is_error).await;
        send_tool_result(
            &sink,
            &call,
            &(id.clone(), output.clone(), *is_error, images.clone()),
        )
        .await;
    }
    results
}

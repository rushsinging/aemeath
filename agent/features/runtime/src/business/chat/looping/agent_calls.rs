use crate::business::agent::{ToolCall, ToolExecution};
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::tools::{
    run_post_tool_hooks, send_tool_call_status, send_tool_result,
};
use crate::business::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use hook::api::{HookData, ToolHookData};
use share::config::hooks::HookEvent;
use share::tool::ToolOutcome;
use std::path::Path;
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
    workspace_root: &Path,
) -> Vec<ToolExecution>
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
                let workspace_root = workspace_root.to_path_buf();
                async move {
                    execute_one_agent(
                        &context,
                        call,
                        sink,
                        hook_ui,
                        hook_runner,
                        registry_ref,
                        &mut ag_ctx,
                        &workspace_root,
                    )
                    .await
                }
            })
            .collect();
        let batch_results: Vec<Vec<ToolExecution>> = futures::future::join_all(agent_futures).await;
        agent_results.extend(batch_results.into_iter().flatten());
    }
    agent_results
}

#[allow(clippy::too_many_arguments)]
async fn execute_one_agent<S>(
    context: &RuntimeTurnContext,
    call: ToolCall,
    sink: S,
    hook_ui: HookUi<S>,
    hook_runner: hook::api::HookRunner,
    registry: Arc<ToolRegistry>,
    ag_ctx: &mut ToolExecutionContext,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing start: kind=agent tool_name={} runtime_id={} provider_id={} index={} input_len={}",
        call.name,
        call.id,
        call.provider_id,
        call.index,
        call.input.to_string().len(),
    );
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
            workspace_root,
        )
        .await;
    if let Some(blocked_result) = pre_results.iter().find(|r| r.blocked) {
        log::debug!(target: crate::LOG_TARGET,
            "pretooluse timing blocked: kind=agent tool_name={} runtime_id={} provider_id={} exit_code={:?} error_present={}",
            call.name,
            call.id,
            call.provider_id,
            blocked_result.exit_code,
            blocked_result.error.as_ref().is_some_and(|value| !value.is_empty()),
        );
        let error_detail = blocked_result
            .error
            .as_deref()
            .unwrap_or("Blocked by PreToolUse hook");
        let result = ToolExecution::new(&call, ToolOutcome::error(error_detail));
        send_tool_result(&sink, context, &result).await;
        return vec![result];
    }
    log::debug!(target: crate::LOG_TARGET,
        "pretooluse timing approved: kind=agent tool_name={} runtime_id={} provider_id={} hook_count={}",
        call.name,
        call.id,
        call.provider_id,
        pre_results.len(),
    );
    send_tool_call_status(&sink, context, &call, RuntimeToolCallStatus::Ready).await;
    send_tool_call_status(&sink, context, &call, RuntimeToolCallStatus::Running).await;
    log::debug!(target: crate::LOG_TARGET,
        "tool execution timing running_sent: kind=agent tool_name={} runtime_id={} provider_id={}",
        call.name,
        call.id,
        call.provider_id,
    );

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
    let workspace = project::api::WorkspacePersist::snapshot(ag_ctx.workspace.as_ref());
    let _ = sink
        .send_event(RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base: workspace.path_base.clone(),
            workspace_root: workspace.workspace_root.clone(),
            workspace,
        })
        .await;
    let execution = ToolExecution::new(&call, ToolOutcome::from_tool_result(result));
    ag_ctx.progress_tx = None;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    run_post_tool_hooks(
        &sink,
        &hook_ui,
        &hook_runner,
        &call,
        &execution.outcome.text,
        execution.outcome.is_error,
        workspace_root,
    )
    .await;
    send_tool_result(&sink, context, &execution).await;
    vec![execution]
}

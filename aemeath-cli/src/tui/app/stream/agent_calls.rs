use crate::tui::app::stream::hook_ui::HookUi;
use crate::tui::app::stream::tools::{run_post_tool_hooks, send_tool_result, UiToolResult};
use crate::tui::app::UiEvent;
use aemeath_core::agent::ToolCall;
use aemeath_core::config::hooks::HookEvent;
use aemeath_core::hook::{HookData, ToolHookData};
use aemeath_core::tool::ToolRegistry;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// A handle to a spawned agent task that can be awaited later.
pub(crate) struct PendingAgent {
    pub tool_call_ids: Vec<String>,
    pub handle: tokio::task::JoinHandle<Vec<UiToolResult>>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_agent_calls(
    agent_approved: &[&ToolCall],
    registry: &Arc<ToolRegistry>,
    agent_ctx: &aemeath_core::tool::ToolContext,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
    max_agent_concurrency: usize,
    interrupted: &Arc<AtomicBool>,
) -> Vec<UiToolResult> {
    let batch_size = max_agent_concurrency.max(1);
    let mut agent_results = Vec::new();
    for batch in agent_approved.chunks(batch_size) {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }
        for call in batch {
            let _ = tx
                .send(UiEvent::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    summary: call.input.to_string(),
                })
                .await;
        }
        let agent_futures: Vec<_> = batch
            .iter()
            .map(|call| {
                let call = ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                };
                let tx = tx.clone();
                let hook_ui = hook_ui.clone();
                let mut ag_ctx = agent_ctx.clone();
                let hook_runner = hook_runner.clone();
                let registry_ref = registry.clone();
                async move {
                    execute_one_agent(call, tx, hook_ui, hook_runner, registry_ref, &mut ag_ctx)
                        .await
                }
            })
            .collect();
        let batch_results: Vec<Vec<UiToolResult>> = futures::future::join_all(agent_futures).await;
        agent_results.extend(batch_results.into_iter().flatten());
    }
    agent_results
}

/// Spawn agent calls as background tasks without blocking.
/// Returns a `PendingAgent` handle that can be awaited later.
/// Also sends ToolCall UI events so the user sees what's queued.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_agent_calls(
    agent_calls: &[&ToolCall],
    registry: &Arc<ToolRegistry>,
    agent_ctx: &aemeath_core::tool::ToolContext,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
) -> PendingAgent {
    let tool_call_ids: Vec<String> = agent_calls.iter().map(|c| c.id.clone()).collect();

    // Collect owned copies of tool calls for the spawned task
    let owned_calls: Vec<ToolCall> = agent_calls
        .iter()
        .map(|c| ToolCall {
            id: c.id.clone(),
            name: c.name.clone(),
            input: c.input.clone(),
        })
        .collect();

    let tx = tx.clone();
    let hook_ui = hook_ui.clone();
    let hook_runner = hook_runner.clone();
    let registry = registry.clone();
    let mut ag_ctx = agent_ctx.clone();

    let handle = tokio::spawn(async move {
        let mut results = Vec::new();
        for call in owned_calls {
            // Send ToolCall UI event
            let _ = tx
                .send(UiEvent::ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    summary: call.input.to_string(),
                })
                .await;
            let r = execute_one_agent(
                call,
                tx.clone(),
                hook_ui.clone(),
                hook_runner.clone(),
                registry.clone(),
                &mut ag_ctx,
            )
            .await;
            results.extend(r);
        }
        results
    });

    PendingAgent {
        tool_call_ids,
        handle,
    }
}

/// Wait for all pending agents to complete and collect their results.
/// Also replaces placeholder tool results in the messages with real ones.
pub(crate) async fn drain_pending_agents(
    pending: &mut Vec<PendingAgent>,
    tx: &mpsc::Sender<UiEvent>,
) -> Vec<UiToolResult> {
    let mut all_results = Vec::new();
    for pending_agent in pending.drain(..) {
        match pending_agent.handle.await {
            Ok(results) => {
                for result in &results {
                    let _ = tx
                        .send(UiEvent::ToolResult {
                            id: result.0.clone(),
                            tool_name: "Agent".to_string(),
                            output: result.1.clone(),
                            is_error: result.2,
                            images: result.3.clone(),
                        })
                        .await;
                }
                all_results.extend(results);
            }
            Err(e) => {
                log::warn!("Pending agent task failed: {e}");
                for id in &pending_agent.tool_call_ids {
                    let result = (
                        id.clone(),
                        format!("Agent task failed: {e}"),
                        true,
                        vec![],
                    );
                    let _ = tx
                        .send(UiEvent::ToolResult {
                            id: id.clone(),
                            tool_name: "Agent".to_string(),
                            output: result.1.clone(),
                            is_error: true,
                            images: vec![],
                        })
                        .await;
                    all_results.push(result);
                }
            }
        }
    }
    all_results
}

async fn execute_one_agent(
    call: ToolCall,
    tx: mpsc::Sender<UiEvent>,
    hook_ui: HookUi,
    hook_runner: aemeath_core::hook::HookRunner,
    registry: Arc<ToolRegistry>,
    ag_ctx: &mut aemeath_core::tool::ToolContext,
) -> Vec<UiToolResult> {
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
        send_tool_result(&tx, &call, &result).await;
        return vec![result];
    }

    let (prog_tx, mut prog_rx) =
        tokio::sync::mpsc::channel::<aemeath_core::tool::AgentProgressEvent>(32);
    ag_ctx.progress_tx = Some(prog_tx);
    let call_id = call.id.clone();
    let ui_tx = tx.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(event) = prog_rx.recv().await {
            let _ = ui_tx
                .send(UiEvent::AgentProgress {
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
    let results = vec![(
        call.id.clone(),
        result.output,
        result.is_error,
        result.images,
    )];
    ag_ctx.progress_tx = None;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), forward_handle).await;

    for (id, output, is_error, images) in &results {
        run_post_tool_hooks(&tx, &hook_ui, &hook_runner, &call, output, *is_error).await;
        send_tool_result(
            &tx,
            &call,
            &(id.clone(), output.clone(), *is_error, images.clone()),
        )
        .await;
    }
    results
}

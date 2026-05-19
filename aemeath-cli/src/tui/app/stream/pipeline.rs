//! Agent pipeline mode: when the LLM returns only Agent tool calls (common
//! with providers like DeepSeek / Zhipu that don't support `parallel_tool_calls`),
//! we spawn each Agent call in the background immediately and return a
//! placeholder tool result so the LLM continues generating more Agent calls.
//!
//! All pending agents are collected and awaited when:
//! - A non-Agent tool call round appears
//! - The main loop ends (EndTurn, error, interrupt, stall)

use crate::tui::app::stream::agent_calls::{drain_pending_agents, spawn_agent_calls, PendingAgent};
use crate::tui::app::stream::hook_ui::HookUi;
use crate::tui::app::stream::tools::{deny_tool_calls, tool_results_for_api, UiToolResult};
use crate::tui::app::UiEvent;
use aemeath_core::agent::{Agent, ToolCall};
use aemeath_core::message::Message;
use aemeath_core::tool::{ImageData, ToolRegistry};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Whether all tool calls in the list are Agent calls.
pub(crate) fn is_all_agent_calls(tool_calls: &[ToolCall]) -> bool {
    !tool_calls.is_empty() && tool_calls.iter().all(|tc| tc.name == "Agent")
}

/// Build placeholder tool results for pipeline mode — tells the LLM the agent
/// was dispatched so it can continue generating more tool calls.
fn placeholder_tool_results(tool_calls: &[ToolCall]) -> Vec<UiToolResult> {
    tool_calls
        .iter()
        .map(|tc| {
            (
                tc.id.clone(),
                "Agent dispatched in parallel pipeline — collecting more agents...".to_string(),
                false,
                Vec::<ImageData>::new(),
            )
        })
        .collect()
}

/// Handle an all-Agent tool call round in pipeline mode.
/// Spawns agents in the background and pushes placeholder tool results
/// to `messages` so the LLM can continue generating.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_pipeline_round(
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    allow_all: bool,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
    agent: &Agent<'_>,
    messages: &mut Vec<Message>,
    session_id: &str,
    pending_agents: &mut Vec<PendingAgent>,
    turn_count: usize,
) {
    log::info!(
        "[agent-pipeline] turn={turn_count}: spawning {} agent(s) in pipeline, total pending={}",
        tool_calls.len(),
        pending_agents.len() + 1,
    );

    let (approved, denied) =
        crate::tui::app::stream::permissions::split_approved_calls(tool_calls, registry, allow_all);
    let denied_results = deny_tool_calls(&denied, tx, hook_ui, hook_runner).await;

    if !approved.is_empty() {
        let pending = spawn_agent_calls(&approved, registry, &agent.ctx, tx, hook_ui, hook_runner);
        pending_agents.push(pending);
    }

    // Build placeholder results for approved calls + real results for denied
    let placeholder = placeholder_tool_results(tool_calls);
    messages.push(tool_results_for_api(
        denied_results.into_iter().chain(placeholder).collect(),
        session_id,
    ));
    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
}

/// Drain all pending agents (called before non-Agent rounds or at loop end).
pub(crate) async fn flush_pending_agents(
    pending_agents: &mut Vec<PendingAgent>,
    tx: &mpsc::Sender<UiEvent>,
) {
    if !pending_agents.is_empty() {
        log::info!(
            "[agent-pipeline] flushing {} pending agent(s)",
            pending_agents.len(),
        );
        drain_pending_agents(pending_agents, tx).await;
    }
}

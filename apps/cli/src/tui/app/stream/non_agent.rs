use crate::tui::app::stream::hook_ui::HookUi;
use crate::tui::app::UiEvent;
use kernel::agent::{Agent, ToolCall};
use kernel::config::hooks::HookEvent;
use kernel::hook::{HookData, ToolHookData};
use kernel::logging::JsonLogger;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::tools::{
    emit_json_hook_context, log_tool_result, run_post_tool_hooks, send_tool_result, UiToolResult,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_non_agent(
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &kernel::hook::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    non_agent_calls: &[ToolCall],
) -> Vec<UiToolResult> {
    let other_calls: Vec<&ToolCall> = non_agent_calls
        .iter()
        .filter(|c| c.name != "AskUserQuestion")
        .collect();

    if other_calls.is_empty() {
        return Vec::new();
    }

    if other_calls.len() == 1 {
        return execute_one_non_agent(
            agent,
            tx,
            hook_ui,
            hook_runner,
            json_logger,
            turn_count,
            client_model,
            other_calls[0],
        )
        .await;
    }

    execute_multiple_non_agent(
        agent,
        tx,
        hook_ui,
        hook_runner,
        json_logger,
        turn_count,
        client_model,
        &other_calls,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_multiple_non_agent(
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &kernel::hook::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    other_calls: &[&ToolCall],
) -> Vec<UiToolResult> {
    let total_len = other_calls.len();
    let mut results: Vec<Option<UiToolResult>> = vec![None; total_len];
    let (concurrent_positions, sequential_positions) = partition_calls(agent, other_calls);

    if !concurrent_positions.is_empty() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(agent.ctx.max_tool_concurrency));
        let futures: Vec<_> = concurrent_positions
            .iter()
            .map(|&pos| {
                let call = other_calls[pos];
                let tx = tx.clone();
                let hook_ui = hook_ui.clone();
                let hook_runner = hook_runner.clone();
                let json_logger = json_logger.clone();
                let sem = semaphore.clone();
                async move {
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    let result = execute_one_non_agent(
                        agent,
                        &tx,
                        &hook_ui,
                        &hook_runner,
                        &json_logger,
                        turn_count,
                        client_model,
                        call,
                    )
                    .await;
                    (pos, result)
                }
            })
            .collect();
        for (pos, result_vec) in futures::future::join_all(futures).await {
            if let Some(r) = result_vec.into_iter().next() {
                results[pos] = Some(r);
            }
        }
    }

    for &pos in &sequential_positions {
        let call = other_calls[pos];
        let result_vec = execute_one_non_agent(
            agent,
            tx,
            hook_ui,
            hook_runner,
            json_logger,
            turn_count,
            client_model,
            call,
        )
        .await;
        if let Some(r) = result_vec.into_iter().next() {
            results[pos] = Some(r);
        }
    }

    results
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            r.unwrap_or_else(|| {
                panic!("execute_non_agent: result slot {i} was not filled — this is a bug")
            })
        })
        .collect()
}

fn partition_calls(agent: &Agent<'_>, calls: &[&ToolCall]) -> (Vec<usize>, Vec<usize>) {
    let mut concurrent_positions = Vec::new();
    let mut sequential_positions = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        let is_safe = agent
            .registry
            .get(&call.name)
            .map(|t| t.is_concurrency_safe())
            .unwrap_or(false);
        if is_safe {
            concurrent_positions.push(i);
        } else {
            sequential_positions.push(i);
        }
    }
    (concurrent_positions, sequential_positions)
}

#[allow(clippy::too_many_arguments)]
async fn execute_one_non_agent(
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &kernel::hook::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    call: &ToolCall,
) -> Vec<UiToolResult> {
    let _ = hook_ui
        .run_plain(
            hook_runner,
            HookEvent::PermissionRequest,
            Some(&call.name),
            HookData::Permission(kernel::hook::PermissionHookData {
                tool_name: call.name.clone(),
                permission_rule: "auto".to_string(),
            }),
        )
        .await;
    let owned_call = ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        input: call.input.clone(),
    };
    let pre_results = hook_ui
        .run_plain(
            hook_runner,
            HookEvent::PreToolUse,
            Some(&owned_call.name),
            HookData::Tool(ToolHookData {
                tool_name: owned_call.name.clone(),
                tool_input: owned_call.input.clone(),
                tool_output: None,
                is_error: None,
            }),
        )
        .await;
    if pre_results.iter().any(|r| r.blocked) {
        let result = (
            owned_call.id.clone(),
            "Blocked by PreToolUse hook".to_string(),
            true,
            Vec::new(),
        );
        send_tool_result(tx, &owned_call, &result).await;
        return vec![result];
    }
    let exec_results = agent.execute_tools(std::slice::from_ref(&owned_call)).await;
    let working_root = agent.ctx.current_working_root();
    hook_runner.set_project_dir(working_root.display().to_string());
    let _ = tx
        .send(crate::tui::app::status_context_for_workspace(
            kernel::worktree::workspace_context_from_tool_context(&agent.ctx),
        ))
        .await;
    let mut out = Vec::new();
    for (id, output, is_error, images) in exec_results {
        log_tool_result(
            json_logger,
            turn_count,
            client_model,
            &id,
            &owned_call.name,
            is_error,
            &output,
        );
        run_post_tool_hooks(tx, hook_ui, hook_runner, &owned_call, &output, is_error).await;
        run_task_hooks(tx, hook_ui, hook_runner, &owned_call, &output, is_error).await;
        let result = (id, output, is_error, images);
        send_tool_result(tx, &owned_call, &result).await;
        out.push(result);
    }
    out
}

async fn run_task_hooks(
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &kernel::hook::HookRunner,
    call: &ToolCall,
    output: &str,
    is_error: bool,
) {
    if !is_error && call.name == "TaskCreate" {
        emit_json_hook_context(
            tx,
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::TaskCreated,
                    None,
                    HookData::Tool(ToolHookData {
                        tool_name: "TaskCreate".to_string(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(false),
                    }),
                )
                .await,
        )
        .await;
    }
    if !is_error && call.name == "TaskUpdate" && output.contains("Status: Completed") {
        emit_json_hook_context(
            tx,
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::TaskCompleted,
                    None,
                    HookData::Tool(ToolHookData {
                        tool_name: "TaskUpdate".to_string(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(false),
                    }),
                )
                .await,
        )
        .await;
    }
}

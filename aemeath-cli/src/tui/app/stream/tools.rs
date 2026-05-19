use crate::tui::app::stream::agent_calls::execute_agent_calls;
use crate::tui::app::stream::ask_user::ask_user;
use crate::tui::app::stream::hook_ui::HookUi;
use crate::tui::app::stream::permissions::split_approved_calls;
use crate::tui::app::UiEvent;
use aemeath_core::agent::{Agent, ToolCall};
use aemeath_core::config::hooks::HookEvent;
use aemeath_core::hook::{HookData, ToolHookData};
use aemeath_core::logging::JsonLogger;
use aemeath_core::tool::{ImageData, ToolRegistry};
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) type UiToolResult = (String, String, bool, Vec<ImageData>);

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_round(
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    allow_all: bool,
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    max_agent_concurrency: usize,
    interrupted: &Arc<std::sync::atomic::AtomicBool>,
) -> Vec<UiToolResult> {
    let (approved, denied) = split_approved_calls(tool_calls, registry, allow_all);
    let denied_results = deny_tool_calls(&denied, tx, hook_ui, hook_runner).await;

    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) =
        approved.into_iter().partition(|c| c.name == "Agent");
    let non_agent_calls: Vec<ToolCall> = non_agent_approved
        .into_iter()
        .map(|c| ToolCall {
            id: c.id.clone(),
            name: c.name.clone(),
            input: c.input.clone(),
        })
        .collect();

    let ask_user_results = ask_user(tx, hook_ui, hook_runner, &non_agent_calls).await;
    let non_agent_results = execute_non_agent(
        agent,
        tx,
        hook_ui,
        hook_runner,
        json_logger,
        turn_count,
        client_model,
        &non_agent_calls,
    )
    .await;
    let agent_results = execute_agent_calls(
        &agent_approved,
        registry,
        &agent.ctx,
        tx,
        hook_ui,
        hook_runner,
        max_agent_concurrency,
        interrupted,
    )
    .await;

    ask_user_results
        .into_iter()
        .chain(non_agent_results.into_iter())
        .chain(agent_results.into_iter())
        .chain(denied_results.into_iter())
        .collect()
}

pub(crate) async fn deny_tool_calls(
    denied: &[&ToolCall],
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
) -> Vec<UiToolResult> {
    let mut denied_results = Vec::new();
    for call in denied {
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionDenied,
                Some(&call.name),
                HookData::Permission(aemeath_core::hook::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "deny".to_string(),
                }),
            )
            .await;
        let result = (
            call.id.clone(),
            format!(
                "Tool {} denied: use --allow-all to permit write operations",
                call.name
            ),
            true,
            Vec::new(),
        );
        send_tool_result(tx, call, &result).await;
        denied_results.push(result);
    }
    denied_results
}

#[allow(clippy::too_many_arguments)]
async fn execute_non_agent(
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    non_agent_calls: &[ToolCall],
) -> Vec<UiToolResult> {
    let other_calls: Vec<&ToolCall> = non_agent_calls
        .iter()
        .filter(|c| c.name != "AskUserQuestion")
        .collect();
    // Send all ToolCall UI events upfront so the user sees what's queued
    for call in &other_calls {
        let _ = tx
            .send(UiEvent::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                summary: call.input.to_string(),
            })
            .await;
    }

    if other_calls.is_empty() {
        return Vec::new();
    }

    // Single tool call — skip grouping overhead
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

    // Multiple calls: partition into concurrent-safe vs sequential,
    // preserving original order via position tracking.
    let total_len = other_calls.len();
    let mut results: Vec<Option<UiToolResult>> = vec![None; total_len];

    let mut concurrent_positions: Vec<usize> = Vec::new();
    let mut sequential_positions: Vec<usize> = Vec::new();
    for (i, call) in other_calls.iter().enumerate() {
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

    // Execute concurrent-safe tools in parallel
    if !concurrent_positions.is_empty() {
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(
            agent.ctx.max_tool_concurrency,
        ));
        let futures: Vec<_> = concurrent_positions
            .iter()
            .filter_map(|&pos| {
                let call = other_calls[pos];
                let agent_ref = agent;
                let tx = tx.clone();
                let hook_ui = hook_ui.clone();
                let hook_runner = hook_runner.clone();
                let json_logger = json_logger.clone();
                let sem = semaphore.clone();
                Some(async move {
                    let _permit = sem.acquire().await.expect("semaphore closed");
                    let result = execute_one_non_agent(
                        agent_ref,
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
                })
            })
            .collect();
        let concurrent_results = futures::future::join_all(futures).await;
        for (pos, result_vec) in concurrent_results {
            // Each call produces exactly one result
            if let Some(r) = result_vec.into_iter().next() {
                results[pos] = Some(r);
            }
        }
    }

    // Execute non-concurrent-safe tools sequentially
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
                panic!(
                    "execute_non_agent: result slot {i} was not filled — this is a bug"
                )
            })
        })
        .collect()
}

/// Execute a single non-agent tool call (hook chain + execute + post hooks + UI result).
#[allow(clippy::too_many_arguments)]
async fn execute_one_non_agent(
    agent: &Agent<'_>,
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
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
            HookData::Permission(aemeath_core::hook::PermissionHookData {
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
    let exec_results = agent
        .execute_tools(std::slice::from_ref(&owned_call))
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

pub(crate) async fn run_post_tool_hooks(
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
    call: &ToolCall,
    output: &str,
    is_error: bool,
) {
    emit_json_hook_context(
        tx,
        hook_ui
            .run_json(
                hook_runner,
                HookEvent::PostToolUse,
                Some(&call.name),
                HookData::Tool(ToolHookData {
                    tool_name: call.name.clone(),
                    tool_input: call.input.clone(),
                    tool_output: Some(output.to_string()),
                    is_error: Some(is_error),
                }),
            )
            .await,
    )
    .await;
    if is_error {
        emit_json_hook_context(
            tx,
            hook_ui
                .run_json(
                    hook_runner,
                    HookEvent::PostToolUseFailure,
                    Some(&call.name),
                    HookData::Tool(ToolHookData {
                        tool_name: call.name.clone(),
                        tool_input: call.input.clone(),
                        tool_output: Some(output.to_string()),
                        is_error: Some(is_error),
                    }),
                )
                .await,
        )
        .await;
    }
}

async fn run_task_hooks(
    tx: &mpsc::Sender<UiEvent>,
    hook_ui: &HookUi,
    hook_runner: &aemeath_core::hook::HookRunner,
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

async fn emit_json_hook_context(
    tx: &mpsc::Sender<UiEvent>,
    hook_results: Vec<(
        aemeath_core::config::hooks::HookEntry,
        aemeath_core::hook::HookResult,
        Option<aemeath_core::hook::HookJsonOutput>,
    )>,
) {
    for (_entry, _result, json_output) in hook_results {
        if let Some(json) = json_output {
            if let Some(ctx) = json.additional_context {
                let _ = tx.send(UiEvent::SystemMessage(ctx)).await;
            }
            if let Some(msg) = json.system_message {
                let _ = tx.send(UiEvent::SystemMessage(msg)).await;
            }
        }
    }
}

pub(crate) async fn send_tool_result(
    tx: &mpsc::Sender<UiEvent>,
    call: &ToolCall,
    result: &UiToolResult,
) {
    let _ = tx
        .send(UiEvent::ToolResult {
            id: result.0.clone(),
            tool_name: call.name.clone(),
            output: result.1.clone(),
            is_error: result.2,
            images: result.3.clone(),
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::tool_results_for_api;
    use aemeath_core::compact::MAX_TOOL_RESULT_CHARS;
    use aemeath_core::message::ContentBlock;

    #[test]
    fn test_tool_results_for_api_persists_oversized_tui_result() {
        let session_id = format!("test-tui-{}", std::process::id());
        let oversized = "x".repeat(MAX_TOOL_RESULT_CHARS + 1);
        let results = vec![("tool-oversized".to_string(), oversized, false, Vec::new())];

        let message = tool_results_for_api(results, &session_id);

        let [ContentBlock::ToolResult { content, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        let text = content.as_str().expect("tool result should be string");
        assert!(text.contains("<persisted-output>"));
        assert!(text.len() < MAX_TOOL_RESULT_CHARS);
        assert!(text.contains(&session_id));
    }
}

pub(crate) fn tool_results_for_api(
    mut results: Vec<UiToolResult>,
    session_id: &str,
) -> aemeath_core::message::Message {
    aemeath_core::tool_result_storage::persist_oversized_results(session_id, &mut results);
    aemeath_core::message::Message::tool_results_rich(results)
}

fn log_tool_result(
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    id: &str,
    tool_name: &str,
    is_error: bool,
    output: &str,
) {
    if let Some(jl) = json_logger {
        let tr_data = serde_json::json!({
            "tool_use_id": id,
            "tool_name": tool_name,
            "is_error": is_error,
            "output": output,
        });
        let _ = jl
            .lock()
            .unwrap()
            .log_tool_result(turn_count, "default", client_model, tr_data);
    }
}

use crate::render::TerminalRenderer;
use ::runtime::api::agent::{Agent, ToolCall, ToolResultTuple};
use ::runtime::api::core::task::{TaskStatus, TaskStore};
use ::runtime::api::core::tool::ToolRegistry;
use ::runtime::api::provider::client::LlmClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use super::tools::ask_permission;

pub(super) async fn pending_task_lines(task_store: &Arc<TaskStore>) -> Vec<String> {
    task_store
        .list()
        .await
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .map(|t| {
            let dep = if t.blocked_by.is_empty() {
                String::new()
            } else {
                format!(" (blocked by #{})", t.blocked_by.join(", #"))
            };
            format!("  ○ #{} {}{}", t.id, t.subject, dep)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_and_render_tools(
    agent: &Agent<'_>,
    registry: &ToolRegistry,
    tool_calls: &[ToolCall],
    call_summaries: &HashMap<String, (String, String)>,
    task_store: &Arc<TaskStore>,
    cancel: &CancellationToken,
    session_id: &str,
    allow_all: bool,
    json_logger: &Option<Arc<Mutex<::runtime::api::storage::logging::JsonLogger>>>,
    client: &LlmClient,
    turn_number: usize,
) -> Vec<ToolResultTuple> {
    let (approved_calls, denied_results) = approve_tool_calls(registry, tool_calls, allow_all);
    let progress_handle = spawn_todo_progress_poller(&approved_calls, task_store, cancel);

    let mut results = agent.execute_tools_filtered(&approved_calls).await;
    results.extend(denied_results);

    if let Some(handle) = progress_handle {
        handle.abort();
    }

    let persisted = ::runtime::api::storage::tool_result_storage::persist_oversized_results(
        session_id,
        &mut results,
    );
    if persisted > 0 {
        println!("[{persisted} tool result(s) persisted to disk]");
    }

    log_tool_results(json_logger, client, turn_number, &results, call_summaries);
    render_tool_results(&results, call_summaries);
    results
}

fn approve_tool_calls<'a>(
    registry: &ToolRegistry,
    tool_calls: &'a [ToolCall],
    allow_all: bool,
) -> (Vec<&'a ToolCall>, Vec<ToolResultTuple>) {
    let mut approved_calls = Vec::new();
    let mut denied_results = Vec::new();

    for call in tool_calls {
        let is_safe = if call.name == "Bash" {
            call.input
                .get("command")
                .and_then(|v| v.as_str())
                .map(::runtime::api::tools::bash::is_readonly_command)
                .unwrap_or(false)
        } else {
            registry
                .get(&call.name)
                .map(|t| t.is_read_only())
                .unwrap_or(false)
        };

        if is_safe || allow_all || ask_permission(&call.name) {
            approved_calls.push(call);
        } else {
            denied_results.push((
                call.id.clone(),
                format!("Tool {} was denied by user", call.name),
                true,
                Vec::new(),
            ));
        }
    }

    (approved_calls, denied_results)
}

fn spawn_todo_progress_poller(
    approved_calls: &[&ToolCall],
    task_store: &Arc<TaskStore>,
    cancel: &CancellationToken,
) -> Option<tokio::task::JoinHandle<()>> {
    if !approved_calls.iter().any(|c| c.name == "TodoRun") {
        return None;
    }

    let store = task_store.clone();
    let cancel_token = cancel.clone();
    Some(tokio::spawn(async move {
        let mut last_statuses: HashMap<String, TaskStatus> = HashMap::new();
        for task in store.list().await {
            last_statuses.insert(task.id.clone(), task.status.clone());
        }

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => break,
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                    update_todo_progress(&store, &mut last_statuses).await;
                }
            }
        }
    }))
}

async fn update_todo_progress(store: &TaskStore, last_statuses: &mut HashMap<String, TaskStatus>) {
    let current = store.list().await;
    for task in &current {
        let changed = last_statuses
            .get(&task.id)
            .map(|status| *status != task.status)
            .unwrap_or(true);
        if !changed {
            continue;
        }

        match task.status {
            TaskStatus::InProgress => {
                let action = task.active_form.as_deref().unwrap_or("Processing");
                eprintln!("  ◐ {} — {}", task.subject, action);
            }
            TaskStatus::Completed => {
                eprintln!("  ✓ {}", task.subject);
            }
            _ => {}
        }
        last_statuses.insert(task.id.clone(), task.status.clone());
    }
}

fn log_tool_results(
    json_logger: &Option<Arc<Mutex<::runtime::api::storage::logging::JsonLogger>>>,
    client: &LlmClient,
    turn_number: usize,
    results: &[ToolResultTuple],
    call_summaries: &HashMap<String, (String, String)>,
) {
    if let Some(jl) = json_logger {
        for (id, output, is_error, _images) in results.iter() {
            let tool_name = call_summaries
                .get(id)
                .map(|(name, _)| name.as_str())
                .unwrap_or("");
            let data = serde_json::json!({
                "tool_use_id": id,
                "tool_name": tool_name,
                "is_error": is_error,
                "output": output,
            });
            let _ = jl.lock().unwrap().log_tool_result(
                turn_number,
                "default",
                client.model_name(),
                data,
            );
        }
    }
}

fn render_tool_results(
    results: &[ToolResultTuple],
    call_summaries: &HashMap<String, (String, String)>,
) {
    for (id, output, is_error, _images) in results.iter() {
        if let Some((name, summary)) = call_summaries.get(id) {
            TerminalRenderer::print_tool_call(name, summary);
        }
        let tool_name = call_summaries
            .get(id)
            .map(|(name, _)| name.as_str())
            .unwrap_or("");
        TerminalRenderer::print_tool_result_with_diff(tool_name, output, *is_error);
    }
}

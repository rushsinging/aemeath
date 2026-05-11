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

async fn deny_tool_calls(
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
    for call in &other_calls {
        let _ = tx
            .send(UiEvent::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                summary: call.input.to_string(),
            })
            .await;
    }

    let mut non_agent_results = Vec::new();
    for call in &other_calls {
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
        let call = ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            input: call.input.clone(),
        };
        let pre_results = hook_ui
            .run_plain(
                hook_runner,
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
            send_tool_result(tx, &call, &result).await;
            non_agent_results.push(result);
            continue;
        }
        let results = agent.execute_tools(std::slice::from_ref(&call)).await;
        for (id, output, is_error, images) in results {
            log_tool_result(
                json_logger,
                turn_count,
                client_model,
                &id,
                &call.name,
                is_error,
                &output,
            );
            run_post_tool_hooks(tx, hook_ui, hook_runner, &call, &output, is_error).await;
            run_task_hooks(tx, hook_ui, hook_runner, &call, &output, is_error).await;
            let result = (id, output, is_error, images);
            send_tool_result(tx, &call, &result).await;
            non_agent_results.push(result);
        }
    }
    non_agent_results
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

use crate::business::agent::{Agent, ToolCall};
use crate::business::chat::looping::agent_calls::execute_agent_calls;
use crate::business::chat::looping::ask_user::ask_user;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::non_agent::execute_non_agent;
use crate::business::chat::looping::permissions::split_approved_calls;
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use hook::api::{HookData, ToolHookData};
use logging::JsonLogger;
use share::config::hooks::HookEvent;
use share::tool::ImageData;
use std::sync::Arc;
use tools::api::ToolRegistry;

pub(crate) type UiToolResult = (
    String,
    String,
    String,
    serde_json::Value,
    bool,
    Vec<ImageData>,
);

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_round<S>(
    context: &RuntimeTurnContext,
    tool_calls: &[ToolCall],
    registry: &Arc<ToolRegistry>,
    allow_all: bool,
    agent: &Agent<'_>,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    json_logger: &Option<Arc<std::sync::Mutex<JsonLogger>>>,
    turn_count: usize,
    client_model: &str,
    max_agent_concurrency: usize,
    interrupted: &Arc<std::sync::atomic::AtomicBool>,
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let (approved, denied) = split_approved_calls(tool_calls, registry, allow_all);
    let denied_results = deny_tool_calls(&denied, sink, context, hook_ui, hook_runner).await;

    // 发送所有 approved calls 的 ToolCall UI 事件，让 pending 占位行尽早原地更新
    for call in &approved {
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCall {
                context: context.clone(),
                id: call.id.clone(),
                provider_id: call.provider_id.clone(),
                name: call.name.clone(),
                index: Some(call.index),
                summary: call.input.to_string(),
            })
            .await;
    }

    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) =
        approved.into_iter().partition(|c| c.name == "Agent");
    let non_agent_calls: Vec<ToolCall> = non_agent_approved
        .into_iter()
        .map(|c| ToolCall {
            id: c.id.clone(),
            provider_id: c.provider_id.clone(),
            name: c.name.clone(),
            index: c.index,
            input: c.input.clone(),
        })
        .collect();

    let ask_user_results = ask_user(context, sink, hook_ui, hook_runner, &non_agent_calls).await;
    let non_agent_results = execute_non_agent(
        context,
        agent,
        sink,
        hook_ui,
        hook_runner,
        json_logger,
        turn_count,
        client_model,
        &non_agent_calls,
    )
    .await;
    let agent_results = execute_agent_calls(
        context,
        &agent_approved,
        registry,
        &agent.ctx,
        sink,
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

async fn deny_tool_calls<S>(
    denied: &[&ToolCall],
    sink: &S,
    context: &RuntimeTurnContext,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let mut denied_results = Vec::new();
    for call in denied {
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionDenied,
                Some(&call.name),
                HookData::Permission(hook::api::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "deny".to_string(),
                }),
            )
            .await;
        // 发送 ToolCall 事件，让 pending 占位行获取 LLM 的 tool_use_id，
        // 后续 ToolResult 中的 mark_tool_header_done 才能精确匹配（Bug #52）。
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCall {
                context: context.clone(),
                id: call.id.clone(),
                provider_id: call.provider_id.clone(),
                name: call.name.clone(),
                index: Some(call.index),
                summary: call.input.to_string(),
            })
            .await;
        let result = (
            call.id.clone(),
            call.provider_id.clone(),
            format!(
                "Tool {} denied: use --allow-all to permit write operations",
                call.name
            ),
            serde_json::json!({
                "status": "error",
                "message": format!(
                    "Tool {} denied: use --allow-all to permit write operations",
                    call.name
                )
            }),
            true,
            Vec::new(),
        );
        send_tool_result(sink, context, call, &result).await;
        denied_results.push(result);
    }
    denied_results
}

pub(crate) async fn run_post_tool_hooks<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    call: &ToolCall,
    output: &str,
    is_error: bool,
) where
    S: ChatEventSink,
{
    emit_json_hook_context(
        sink,
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
            sink,
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

pub(crate) async fn emit_json_hook_context<S>(
    sink: &S,
    hook_results: Vec<(
        share::config::hooks::HookEntry,
        hook::api::HookResult,
        Option<hook::api::HookJsonOutput>,
    )>,
) where
    S: ChatEventSink,
{
    for (_entry, _result, json_output) in hook_results {
        if let Some(json) = json_output {
            if let Some(ctx) = json.additional_context {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(ctx))
                    .await;
            }
            if let Some(msg) = json.system_message {
                let _ = sink
                    .send_event(RuntimeStreamEvent::SystemMessage(msg))
                    .await;
            }
        }
    }
}

pub(crate) async fn send_tool_result<S>(
    sink: &S,
    context: &RuntimeTurnContext,
    call: &ToolCall,
    result: &UiToolResult,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::ToolResult {
            context: context.clone(),
            id: result.0.clone(),
            provider_id: result.1.clone(),
            tool_name: call.name.clone(),
            output: result.2.clone(),
            content: result.3.clone(),
            is_error: result.4,
            images: result.5.clone(),
        })
        .await;
}

pub(crate) fn tool_results_for_api(
    mut results: Vec<UiToolResult>,
    session_id: &str,
) -> share::message::Message {
    let mut provider_results: Vec<_> = results
        .drain(..)
        .map(
            |(_runtime_id, provider_id, output, content, is_error, images)| {
                (provider_id, output, content, is_error, images)
            },
        )
        .collect();
    storage::api::persist_oversized_results(session_id, &mut provider_results);
    share::message::Message::tool_results_rich(provider_results)
}

pub(crate) fn log_tool_result(
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

#[cfg(test)]
mod tests {
    use super::tool_results_for_api;
    use crate::business::compact::MAX_TOOL_RESULT_CHARS;
    use share::message::ContentBlock;

    #[test]
    fn test_tool_results_for_api_uses_provider_id_not_runtime_id() {
        let results = vec![(
            "runtime-id".to_string(),
            "provider-id".to_string(),
            "ok".to_string(),
            serde_json::json!({ "text": "ok" }),
            false,
            Vec::new(),
        )];
        let message = tool_results_for_api(results, "test-provider-id");

        let [ContentBlock::ToolResult { tool_use_id, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        assert_eq!(tool_use_id, "provider-id");
    }

    #[test]
    fn test_tool_results_for_api_persists_oversized_tui_result() {
        let session_id = format!("test-tui-{}", std::process::id());
        let oversized = "x".repeat(MAX_TOOL_RESULT_CHARS + 1);
        let results = vec![(
            "tool-1".to_string(),
            "provider-oversized".to_string(),
            oversized,
            serde_json::json!({ "text": "oversized" }),
            false,
            Vec::new(),
        )];
        let message = tool_results_for_api(results, &session_id);

        let [ContentBlock::ToolResult { content, .. }] = message.content.as_slice() else {
            panic!("expected one tool result");
        };
        let content = match content {
            serde_json::Value::Object(map) => map,
            other => panic!("tool result should be json object, got {other:?}"),
        };
        let text = content
            .get("text")
            .and_then(|value| value.as_str())
            .expect("persisted reference should be in text field");
        assert!(text.contains("<persisted-output>"));
        assert!(text.len() < MAX_TOOL_RESULT_CHARS);
        assert!(text.contains(&session_id));
    }
}

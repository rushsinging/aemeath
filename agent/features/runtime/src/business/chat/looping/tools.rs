// summary 已由 TUI 层从 input 参数组装，runtime 不再生成
use crate::business::agent::{Agent, ToolCall, ToolExecution};
use crate::business::chat::looping::agent_calls::execute_agent_calls;
use crate::business::chat::looping::ask_user::ask_user;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::non_agent::execute_non_agent;
use crate::business::chat::looping::permissions::evaluate_calls;
use crate::business::chat::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use hook::api::{HookData, ToolHookData};

use crate::business::chat::looping::engine::{DeniedCall, PolicyEngine};
use crate::LOG_TARGET;
use sdk::ids::ToolCallId;
use share::config::hooks::HookEvent;
use share::tool::ToolOutcome;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tools::api::ToolRegistry;

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
    max_agent_concurrency: usize,
    cancel: &CancellationToken,
    language: &str,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let path_base = agent.ctx.workspace_read().current_path_base();
    let current_ws_root = agent.ctx.workspace_read().current_workspace_root();
    let engine = PolicyEngine::new(
        &path_base,
        &current_ws_root,
        allow_all,
        &agent.ctx.read_files,
    );

    let (approved, denied) = evaluate_calls(tool_calls, registry, &engine);
    let denied_results =
        deny_tool_calls(&denied, sink, context, hook_ui, hook_runner, workspace_root).await;

    // 发送所有 approved calls 的 ToolCall UI 事件，让 pending 占位行尽早原地更新
    for call in &approved {
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCallUpdate {
                context: context.clone(),
                id: call.id.clone(),
                provider_id: Some(call.provider_id.clone()),
                name: call.name.clone(),
                index: call.index,
                arguments_delta: None,
                arguments: Some(call.input.clone()),
                status: RuntimeToolCallStatus::Ready,
            })
            .await;
    }
    let (agent_approved, non_agent_approved): (Vec<ToolCall>, Vec<ToolCall>) =
        approved.into_iter().partition(|c| c.name == "Agent");

    let ask_user_results = ask_user(
        context,
        sink,
        hook_ui,
        hook_runner,
        &non_agent_approved,
        workspace_root,
    )
    .await;
    let non_agent_results = execute_non_agent(
        context,
        agent,
        sink,
        hook_ui,
        hook_runner,
        &non_agent_approved,
        language,
        workspace_root,
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
        cancel,
        workspace_root,
    )
    .await;

    ask_user_results
        .into_iter()
        .chain(non_agent_results)
        .chain(agent_results)
        .chain(denied_results)
        .collect()
}

async fn deny_tool_calls<S>(
    denied: &[DeniedCall],
    sink: &S,
    context: &RuntimeTurnContext,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    let mut denied_results = Vec::new();
    for call in denied {
        log::warn!(
            target: LOG_TARGET,
            "tool call denied by policy: name={}, reason={}, runtime_id={}, provider_id={}",
            call.name, call.reason, call.id, call.provider_id,
        );
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionDenied,
                Some(&call.name),
                HookData::Permission(hook::api::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "deny".to_string(),
                }),
                workspace_root,
            )
            .await;
        // 发送 ToolCall 事件，让 pending 占位行获取 LLM 的 tool_use_id，
        // 后续 ToolResult 中的 mark_tool_header_done 才能精确匹配（Bug #52）。
        let call_id = sdk::ids::ToolCallId::from_legacy_or_new(&call.id);
        let _ = sink
            .send_event(RuntimeStreamEvent::ToolCallUpdate {
                context: context.clone(),
                id: call_id.clone(),
                provider_id: None,
                name: call.name.clone(),
                index: 0,
                arguments_delta: None,
                arguments: None,
                status: RuntimeToolCallStatus::Ready,
            })
            .await;
        // 保持原 wire 形态 {"status":"error","message":...}（与 deny 路径历史一致）。
        let outcome = ToolOutcome {
            text: call.reason.clone(),
            data: serde_json::json!({
                "status": "error",
                "message": call.reason,
            }),
            is_error: true,
            images: Vec::new(),
        };
        let execution = ToolExecution::from_parts(
            call_id,
            call.provider_id.clone(),
            call.name.clone(),
            outcome,
        );
        send_tool_result(sink, context, &execution).await;
        denied_results.push(execution);
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
    workspace_root: &Path,
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
                workspace_root,
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
                    workspace_root,
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
    execution: &ToolExecution,
) where
    S: ChatEventSink,
{
    let _ = sink
        .send_event(RuntimeStreamEvent::ToolResult {
            context: context.clone(),
            id: execution.call_id.clone(),
            provider_id: execution.provider_id.clone(),
            tool_name: execution.tool_name.clone(),
            output: execution.outcome.text.clone(),
            content: execution.outcome.data.clone(),
            is_error: execution.outcome.is_error,
            images: execution.outcome.images.clone(),
        })
        .await;
}

pub(crate) fn tool_results_for_api(
    results: Vec<ToolExecution>,
    session_id: &str,
) -> share::message::Message {
    let error_count = results.iter().filter(|ex| ex.outcome.is_error).count();
    log::debug!(
        target: LOG_TARGET,
        "tool_results_for_api: {} typed ToolExecution(s) → wire ({} error)",
        results.len(),
        error_count
    );
    let mut provider_results: Vec<_> = results
        .into_iter()
        .map(|ex| {
            (
                ex.provider_id,
                ex.outcome.text,
                ex.outcome.data,
                ex.outcome.is_error,
                ex.outcome.images,
            )
        })
        .collect();
    storage::api::persist_oversized_results(session_id, &mut provider_results);
    share::message::Message::tool_results_rich(provider_results)
}

pub(crate) fn log_tool_result(id: &ToolCallId, tool_name: &str, is_error: bool, output: &str) {
    let tr_data = serde_json::json!({
        "tool_use_id": id.to_string(),
        "tool_name": tool_name,
        "is_error": is_error,
        "output": output,
    });
    log::debug!(
        target: LOG_TARGET,
        "tool_result: {}",
        serde_json::to_string(&tr_data).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests {
    use super::tool_results_for_api;
    use crate::business::agent::ToolExecution;
    use crate::business::compact::MAX_TOOL_RESULT_CHARS;
    use sdk::ids::ToolCallId;
    use share::message::ContentBlock;
    use share::tool::ToolOutcome;

    #[test]
    fn test_tool_results_for_api_uses_provider_id_not_runtime_id() {
        let results = vec![ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-id".to_string(),
            "Bash".to_string(),
            ToolOutcome::new("ok", serde_json::json!({ "text": "ok" }), Vec::new()),
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
        let results = vec![ToolExecution::from_parts(
            ToolCallId::new_v7(),
            "provider-oversized".to_string(),
            "Bash".to_string(),
            ToolOutcome::new(
                oversized,
                serde_json::json!({ "text": "oversized" }),
                Vec::new(),
            ),
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

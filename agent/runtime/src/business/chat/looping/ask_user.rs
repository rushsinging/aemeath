use crate::api::agent::ToolCall;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::tools::{send_tool_result, UiToolResult};
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent};
use hook::api::HookData;
use share::config::hooks::HookEvent;

pub(crate) async fn ask_user<S>(
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    non_agent_calls: &[ToolCall],
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let mut ask_user_results = Vec::new();
    let ask_calls: Vec<&ToolCall> = non_agent_calls
        .iter()
        .filter(|c| c.name == "AskUserQuestion")
        .collect();
    for call in &ask_calls {
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionRequest,
                Some(&call.name),
                HookData::Permission(hook::api::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "manual".to_string(),
                }),
            )
            .await;
        let question = call
            .input
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let options: Vec<String> = call
            .input
            .get("options")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let allow_free_input = call
            .input
            .get("allow_free_input")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let multi_select = call
            .input
            .get("multi_select")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let default = call
            .input
            .get("default")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<String>();
        let _ = sink
            .send_event(RuntimeStreamEvent::AskUser {
                id: call.id.clone(),
                question,
                options,
                allow_free_input,
                multi_select,
                default: default.clone(),
                reply_tx,
            })
            .await;
        let answer = match reply_rx.await {
            Ok(a) if !a.is_empty() => a,
            _ => default.unwrap_or_default(),
        };
        let result = (call.id.clone(), answer, false, Vec::new());
        send_tool_result(sink, call, &result).await;
        ask_user_results.push(result);
    }
    ask_user_results
}

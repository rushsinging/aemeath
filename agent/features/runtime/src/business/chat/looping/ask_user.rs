use crate::business::agent::ToolCall;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::tools::{send_tool_result, UiToolResult};
use crate::business::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use hook::api::HookData;
use sdk::{AskUserQuestionItem, OptionItem};
use share::config::hooks::HookEvent;

pub(crate) async fn ask_user<S>(
    context: &RuntimeTurnContext,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    non_agent_calls: &[ToolCall],
) -> Vec<UiToolResult>
where
    S: ChatEventSink,
{
    let ask_calls: Vec<&ToolCall> = non_agent_calls
        .iter()
        .filter(|c| c.name == "AskUserQuestion")
        .collect();

    if ask_calls.is_empty() {
        return Vec::new();
    }

    // 对每个 call 运行 PermissionRequest hook（保持现有逻辑不变）
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
    }

    // 收集所有问题为 Vec<AskUserQuestionItem>
    let items: Vec<AskUserQuestionItem> = ask_calls
        .iter()
        .map(|call| {
            let question = call
                .input
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let options: Vec<OptionItem> = call
                .input
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| {
                            // 新格式：{ "title": "...", "description": "..." }
                            if v.is_object() {
                                let title = v.get("title").and_then(|t| t.as_str())?;
                                let description = v.get("description").and_then(|d| d.as_str());
                                Some(OptionItem {
                                    title: title.to_string(),
                                    description: description.map(|d| d.to_string()),
                                })
                            } else {
                                // 兼容旧格式：纯字符串
                                v.as_str().map(|s| OptionItem::title_only(s.to_string()))
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
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
            AskUserQuestionItem {
                id: call.id.to_string(),
                question,
                options,
                multi_select,
                default,
            }
        })
        .collect();

    // 创建单个 oneshot channel，发送单个 AskUserBatch 事件
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<Vec<String>>();
    let _ = sink
        .send_event(RuntimeStreamEvent::AskUserBatch { items, reply_tx })
        .await;

    // 等待用户回答所有问题
    let answers: Vec<String> = reply_rx.await.unwrap_or_default();

    // 收到答案后，逐个 send_tool_result 回传；
    // 答案数量不匹配时用 default 值填充
    let mut ask_user_results = Vec::new();
    for (i, call) in ask_calls.iter().enumerate() {
        let default = call
            .input
            .get("default")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let answer = answers
            .get(i)
            .cloned()
            .filter(|a| !a.is_empty())
            .unwrap_or(default);
        let result = (
            call.id.clone(),
            call.provider_id.clone(),
            answer.clone(),
            serde_json::json!({ "text": answer }),
            false,
            Vec::new(),
        );
        send_tool_result(sink, context, call, &result).await;
        ask_user_results.push(result);
    }
    ask_user_results
}

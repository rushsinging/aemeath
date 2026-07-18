use crate::application::agent::{ToolCall, ToolExecution};
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::tools::send_tool_result;
use crate::application::chat::looping::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use crate::application::suspension_mapping::user_interaction_items;
use hook::api::HookData;
use share::config::hooks::HookEvent;
use std::path::Path;
use tools::{ToolOutcome, ToolSuspension};

/// Runs the existing Runtime-owned AskUser protocol from typed Tool PL values.
///
/// Runtime continues to own tool-call identity, hooks, event delivery, waiting,
/// replies, and ToolExecution materialization. The Tool suspension owns only the
/// pure interaction specification.
pub(crate) async fn ask_user<S>(
    context: &RuntimeTurnContext,
    sink: &S,
    hook_ui: &HookUi<S>,
    hook_runner: &hook::api::HookRunner,
    suspended_calls: &[(&ToolCall, ToolSuspension)],
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    if suspended_calls.is_empty() {
        return Vec::new();
    }

    // 对每个 call 运行 PermissionRequest hook（保持现有逻辑不变）
    for (call, _) in suspended_calls {
        let _ = hook_ui
            .run_plain(
                hook_runner,
                HookEvent::PermissionRequest,
                Some(&call.name),
                HookData::Permission(hook::api::PermissionHookData {
                    tool_name: call.name.clone(),
                    permission_rule: "manual".to_string(),
                }),
                workspace_root,
            )
            .await;
    }

    // Preserve call order, and question order within each typed suspension.
    let items = suspended_calls
        .iter()
        .flat_map(|(call, suspension)| {
            user_interaction_items(call.id.as_ref(), suspension).into_iter()
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
    // 答案数量不匹配时用 default 值填充。
    // AskUser currently publishes one question per call; keeping the answer
    // cursor explicit also makes ordering deterministic if the PL later grows.
    let mut answer_index = 0;
    let mut ask_user_results = Vec::new();
    for (call, suspension) in suspended_calls {
        let ToolSuspension::UserInteraction(spec) = suspension;
        let question = spec
            .questions
            .first()
            .expect("AskUser suspension must contain one question");
        let default = question.default.clone().unwrap_or_default();
        let answer = answers
            .get(answer_index)
            .cloned()
            .filter(|answer| !answer.is_empty())
            .unwrap_or(default);
        answer_index += spec.questions.len();

        let result = ToolExecution::new(
            call,
            ToolOutcome::new(
                answer.clone(),
                serde_json::json!({ "text": answer }),
                Vec::new(),
            ),
        );
        send_tool_result(sink, context, &result).await;
        ask_user_results.push(result);
    }
    ask_user_results
}

#[cfg(test)]
mod tests {
    use super::ask_user;
    use crate::application::agent::ToolCall;
    use crate::application::chat::looping::hook_ui::HookUi;
    use crate::application::chat::looping::{
        ChatEventSink, EventFuture, RuntimeStreamEvent, RuntimeTurnContext,
    };
    use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
    use std::sync::{Arc, Mutex};
    use tools::{ToolSuspension, UserInteractionSpec, UserOption, UserQuestion};

    #[derive(Clone)]
    struct ReplyingSink {
        items: Arc<Mutex<Vec<sdk::AskUserQuestionItem>>>,
        answers: Vec<String>,
    }

    impl ChatEventSink for ReplyingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                if let RuntimeStreamEvent::AskUserBatch { items, reply_tx } = event {
                    *self.items.lock().unwrap() = items;
                    let _ = reply_tx.send(self.answers.clone());
                }
            })
        }

        fn try_send_event(&self, _event: RuntimeStreamEvent) {}
    }

    fn call(id: &str, index: usize) -> ToolCall {
        ToolCall {
            id: ToolCallId::from_legacy_or_new(id),
            provider_id: id.to_string(),
            name: "AskUserQuestion".to_string(),
            index,
            // Deliberately unrelated: Runtime must not parse this raw input.
            input: serde_json::json!({"question": "wrong"}),
        }
    }

    #[tokio::test]
    async fn multiple_typed_ask_user_calls_preserve_order_fields_and_reply_behavior() {
        let calls = [call("first-call", 0), call("second-call", 1)];
        let suspensions = [
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "First typed question",
                vec![UserOption::new(
                    "one",
                    Some("first description".to_string()),
                )],
                true,
                Some("one".to_string()),
            )])),
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "Second typed question",
                vec![UserOption::title_only("two")],
                false,
                Some("fallback".to_string()),
            )])),
        ];
        let suspended_calls = vec![
            (&calls[0], suspensions[0].clone()),
            (&calls[1], suspensions[1].clone()),
        ];
        let sink = ReplyingSink {
            items: Arc::new(Mutex::new(Vec::new())),
            answers: vec![String::new(), "selected two".to_string()],
        };
        let hook_ui = HookUi::new(sink.clone());
        let hook_runner = hook::api::HookRunner::new(Default::default());
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));

        let results = ask_user(
            &context,
            &sink,
            &hook_ui,
            &hook_runner,
            &suspended_calls,
            &std::env::current_dir().unwrap(),
        )
        .await;

        let items = sink.items.lock().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, calls[0].id.to_string());
        assert_eq!(items[0].question, "First typed question");
        assert_eq!(
            items[0].options[0].description.as_deref(),
            Some("first description")
        );
        assert!(items[0].multi_select);
        assert_eq!(items[0].default.as_deref(), Some("one"));
        assert_eq!(items[1].id, calls[1].id.to_string());
        assert_eq!(items[1].question, "Second typed question");
        assert!(!items[1].multi_select);
        assert_eq!(items[1].default.as_deref(), Some("fallback"));
        assert_eq!(results[0].outcome.text, "one");
        assert_eq!(results[1].outcome.text, "selected two");
    }
}

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
    suspended_calls: &[(&ToolCall, ToolSuspension, tools::AuthorizationContext)],
    cancel: &tokio_util::sync::CancellationToken,
    workspace_root: &Path,
) -> Vec<ToolExecution>
where
    S: ChatEventSink,
{
    if suspended_calls.is_empty() {
        return Vec::new();
    }

    // 对每个 call 运行 PermissionRequest hook（保持现有逻辑不变）
    for (call, _, authorization) in suspended_calls {
        if !authorization.enforce_permission_hooks {
            continue;
        }
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
        .flat_map(|(call, suspension, _)| {
            user_interaction_items(call.id.as_ref(), suspension).into_iter()
        })
        .collect();

    // 创建单个 oneshot channel，发送单个 AskUserBatch 事件
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<sdk::AskUserReply>();
    let _ = sink
        .send_event(RuntimeStreamEvent::AskUserBatch { items, reply_tx })
        .await;

    let reply = tokio::select! {
        _ = cancel.cancelled() => sdk::AskUserReply::Cancelled,
        reply = reply_rx => reply.unwrap_or(sdk::AskUserReply::Cancelled),
    };

    let answers = match reply {
        sdk::AskUserReply::Answers(answers) => answers,
        sdk::AskUserReply::Cancelled => {
            let mut results = Vec::with_capacity(suspended_calls.len());
            for (call, _, _) in suspended_calls {
                let result =
                    ToolExecution::new(call, ToolOutcome::error("用户取消了 AskUserQuestion"));
                send_tool_result(sink, context, &result).await;
                results.push(result);
            }
            return results;
        }
    };

    // Each suspended call owns one or more ordered questions. Consume exactly
    // that call's answer slice, applying defaults without crossing call IDs.
    let mut answer_index = 0;
    let mut ask_user_results = Vec::new();
    for (call, suspension, _) in suspended_calls {
        let ToolSuspension::UserInteraction(spec) = suspension;
        if spec.questions.is_empty() {
            let result = ToolExecution::new(
                call,
                ToolOutcome::error("AskUser suspension contains no questions"),
            );
            send_tool_result(sink, context, &result).await;
            ask_user_results.push(result);
            continue;
        }

        let call_answers = spec
            .questions
            .iter()
            .enumerate()
            .map(|(offset, question)| {
                answers
                    .get(answer_index + offset)
                    .cloned()
                    .filter(|answer| !answer.is_empty())
                    .or_else(|| question.default.clone())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        answer_index += spec.questions.len();
        let text = call_answers.join("\n");
        let result = ToolExecution::new(
            call,
            ToolOutcome::new(
                text.clone(),
                serde_json::json!({ "text": text }),
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
        reply: sdk::AskUserReply,
    }

    #[derive(Clone)]
    struct WaitingSink {
        reply_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<sdk::AskUserReply>>>>,
        final_results: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ChatEventSink for WaitingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                match event {
                    RuntimeStreamEvent::AskUserBatch { reply_tx, .. } => {
                        *self.reply_tx.lock().unwrap() = Some(reply_tx);
                    }
                    RuntimeStreamEvent::ToolResult { .. } => {
                        self.final_results
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                    _ => {}
                }
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            if matches!(event, RuntimeStreamEvent::ToolResult { .. }) {
                self.final_results
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    #[derive(Clone)]
    struct CancellingSink {
        cancel: tokio_util::sync::CancellationToken,
    }

    impl ChatEventSink for CancellingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                if matches!(event, RuntimeStreamEvent::AskUserBatch { .. }) {
                    self.cancel.cancel();
                }
            })
        }

        fn try_send_event(&self, _event: RuntimeStreamEvent) {}
    }

    impl ChatEventSink for ReplyingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                if let RuntimeStreamEvent::AskUserBatch { items, reply_tx } = event {
                    *self.items.lock().unwrap() = items;
                    let _ = reply_tx.send(self.reply.clone());
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
                false,
                Some("one".to_string()),
            )])),
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "Second typed question",
                vec![UserOption::title_only("two")],
                false,
                true,
                Some("fallback".to_string()),
            )])),
        ];
        let suspended_calls = vec![
            (
                &calls[0],
                suspensions[0].clone(),
                tools::AuthorizationContext::STANDARD,
            ),
            (
                &calls[1],
                suspensions[1].clone(),
                tools::AuthorizationContext::STANDARD,
            ),
        ];
        let sink = ReplyingSink {
            items: Arc::new(Mutex::new(Vec::new())),
            reply: sdk::AskUserReply::Answers(vec![String::new(), "selected two".to_string()]),
        };
        let hook_ui = HookUi::new(sink.clone());
        let hook_runner = hook::api::HookRunner::new(Default::default());
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let cancel = tokio_util::sync::CancellationToken::new();

        let results = ask_user(
            &context,
            &sink,
            &hook_ui,
            &hook_runner,
            &suspended_calls,
            &cancel,
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
        assert!(!items[0].allow_free_input);
        assert_eq!(items[0].default.as_deref(), Some("one"));
        assert_eq!(items[1].id, calls[1].id.to_string());
        assert_eq!(items[1].question, "Second typed question");
        assert!(!items[1].multi_select);
        assert!(items[1].allow_free_input);
        assert_eq!(items[1].default.as_deref(), Some("fallback"));
        assert_eq!(results[0].outcome.text, "one");
        assert_eq!(results[1].outcome.text, "selected two");
    }

    #[tokio::test]
    async fn reply_must_arrive_before_any_final_tool_result() {
        let calls = [call("waiting-call", 0)];
        let suspension =
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "Continue?",
                vec![],
                false,
                true,
                None,
            )]));
        let suspended_calls = vec![(&calls[0], suspension, tools::AuthorizationContext::STANDARD)];
        let reply_tx = Arc::new(Mutex::new(None));
        let final_results = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sink = WaitingSink {
            reply_tx: reply_tx.clone(),
            final_results: final_results.clone(),
        };
        let hook_ui = HookUi::new(sink.clone());
        let hook_runner = hook::api::HookRunner::new(Default::default());
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let cancel = tokio_util::sync::CancellationToken::new();
        let workspace_root = std::env::current_dir().unwrap();
        let waiting = ask_user(
            &context,
            &sink,
            &hook_ui,
            &hook_runner,
            &suspended_calls,
            &cancel,
            &workspace_root,
        );
        tokio::pin!(waiting);

        tokio::select! {
            _ = &mut waiting => panic!("AskUser completed before receiving a reply"),
            _ = tokio::task::yield_now() => {}
        }
        assert_eq!(final_results.load(std::sync::atomic::Ordering::SeqCst), 0);
        let sender = reply_tx
            .lock()
            .unwrap()
            .take()
            .expect("AskUser request sender");
        sender
            .send(sdk::AskUserReply::Answers(vec!["yes".to_string()]))
            .expect("send AskUser reply");

        let results = waiting.await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome.text, "yes");
        assert_eq!(final_results.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_terminates_each_original_tool_call_as_error() {
        let calls = [call("first-call", 0), call("second-call", 1)];
        let suspension =
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "Continue?",
                vec![],
                false,
                true,
                None,
            )]));
        let suspended_calls = vec![
            (
                &calls[0],
                suspension.clone(),
                tools::AuthorizationContext::STANDARD,
            ),
            (&calls[1], suspension, tools::AuthorizationContext::STANDARD),
        ];
        let cancel = tokio_util::sync::CancellationToken::new();
        let sink = CancellingSink {
            cancel: cancel.clone(),
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
            &cancel,
            &std::env::current_dir().unwrap(),
        )
        .await;

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.outcome.is_error));
        assert!(results
            .iter()
            .all(|result| result.outcome.text.contains("取消")));
    }

    #[tokio::test]
    async fn multiple_questions_are_consumed_without_crossing_tool_calls() {
        let calls = [call("multi-call", 0), call("next-call", 1)];
        let suspensions = [
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![
                UserQuestion::new("First", vec![], false, true, None),
                UserQuestion::new("Second", vec![], false, true, None),
            ])),
            ToolSuspension::UserInteraction(UserInteractionSpec::new(vec![UserQuestion::new(
                "Third",
                vec![],
                false,
                true,
                None,
            )])),
        ];
        let suspended_calls = vec![
            (
                &calls[0],
                suspensions[0].clone(),
                tools::AuthorizationContext::STANDARD,
            ),
            (
                &calls[1],
                suspensions[1].clone(),
                tools::AuthorizationContext::STANDARD,
            ),
        ];
        let sink = ReplyingSink {
            items: Arc::new(Mutex::new(Vec::new())),
            reply: sdk::AskUserReply::Answers(vec!["A".into(), "B".into(), "C".into()]),
        };
        let hook_ui = HookUi::new(sink.clone());
        let hook_runner = hook::api::HookRunner::new(Default::default());
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));
        let cancel = tokio_util::sync::CancellationToken::new();

        let results = ask_user(
            &context,
            &sink,
            &hook_ui,
            &hook_runner,
            &suspended_calls,
            &cancel,
            &std::env::current_dir().unwrap(),
        )
        .await;

        assert_eq!(results[0].outcome.text, "A\nB");
        assert_eq!(results[1].outcome.text, "C");
    }
}

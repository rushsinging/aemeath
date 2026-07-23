use crate::application::main_loop::looping::hook_ui::dispatch_hook;
use crate::application::main_loop::looping::tools::send_tool_result;
use crate::application::main_loop::looping::{
    ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::subagent::{ToolCall, ToolExecution};
use crate::application::suspension_mapping::user_interaction_items;
use hook::{HookInvocation, HookPort, PermissionInput};
use std::sync::Arc;
use tools::{ToolOutcome, ToolSuspension};

/// Runs the existing Runtime-owned AskUser protocol from typed Tool PL values.
///
/// Runtime continues to own tool-call identity, hooks, event delivery, waiting,
/// replies, and ToolExecution materialization. The Tool suspension owns only the
/// pure interaction specification.
/// #944 5B: Legacy AskUser bridge. Production path is `resolve_ask_user_via_bridge`
/// in tools.rs (InteractionRequested + InteractionBridge). This function is dead
/// code, retained for reference until physical removal.
#[allow(dead_code)]
pub(crate) async fn ask_user<S>(
    context: &RuntimeTurnContext,
    sink: &S,
    hook_port: &Arc<dyn HookPort>,
    suspended_calls: &[(&ToolCall, ToolSuspension, tools::AuthorizationContext)],
    cancel: &tokio_util::sync::CancellationToken,
    workspace_root: &std::path::Path,
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
        let _ = dispatch_hook(
            hook_port,
            sink,
            HookInvocation::PermissionRequest(PermissionInput {
                tool_name: call.name.clone(),
                permission_rule: "manual".to_string(),
            }),
            workspace_root,
            cancel,
        )
        .await;
    }

    let items: Vec<_> = suspended_calls
        .iter()
        .flat_map(|(call, suspension, _)| user_interaction_items(call.id.as_ref(), suspension))
        .collect();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let _ = sink
        .send_event(RuntimeStreamEvent::AskUserBatch { items, reply_tx })
        .await;

    // 等待用户回复或取消
    let reply = tokio::select! {
        result = reply_rx => match result {
            Ok(reply) => reply,
            Err(_) => {
                // sender dropped (TUI 断开)
                return suspended_calls
                    .iter()
                    .map(|(call, _, _)| {
                        ToolExecution::new(call, ToolOutcome::error("AskUser channel closed"))
                    })
                    .collect();
            }
        },
        () = cancel.cancelled() => {
            return suspended_calls
                .iter()
                .map(|(call, _, _)| {
                    ToolExecution::new(call, ToolOutcome::error("Cancelled by user"))
                })
                .collect();
        }
    };

    // 组装结果
    let mut results = Vec::new();
    match reply {
        sdk::AskUserReply::Cancelled => {
            for (call, _, _) in suspended_calls {
                results.push(ToolExecution::new(
                    call,
                    ToolOutcome::error("Cancelled by user"),
                ));
            }
        }
        sdk::AskUserReply::Answers(answers) => {
            for (call, suspension, _) in suspended_calls {
                let answer = match suspension {
                    ToolSuspension::UserInteraction(spec) => spec
                        .questions
                        .iter()
                        .enumerate()
                        .map(|(question_seq, question)| {
                            answers
                                .iter()
                                .find(|answer| {
                                    answer.tool_call_id == call.id.as_ref()
                                        && answer.question_seq == question_seq
                                })
                                .map(|answer| answer.answer.clone())
                                .or_else(|| question.default.clone())
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                let outcome = ToolOutcome::new(
                    answer.clone(),
                    serde_json::json!({"answer": answer}),
                    Vec::new(),
                );
                let execution = ToolExecution::new(call, outcome);
                send_tool_result(sink, context, &execution).await;
                results.push(execution);
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::main_loop::looping::{EventFuture, RuntimeStreamEvent};
    use sdk::ids::{ChatId, ChatTurnId, ToolCallId};
    use std::sync::Mutex;

    #[derive(Clone)]
    struct RecordingSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl ChatEventSink for RecordingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            let events = self.events.clone();
            Box::pin(async move {
                events.lock().unwrap().push(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    /// A test HookPort that always returns Continue.
    struct NoOpHookPort;

    #[async_trait::async_trait]
    impl HookPort for NoOpHookPort {
        async fn dispatch(
            &self,
            _invocation: HookInvocation,
            _cancellation: &tokio_util::sync::CancellationToken,
        ) -> hook::HookOutcome {
            hook::HookOutcome::proceed()
        }
    }

    fn noop_hook_port() -> Arc<dyn HookPort> {
        Arc::new(NoOpHookPort)
    }

    fn dummy_call(id: &str) -> ToolCall {
        ToolCall {
            id: ToolCallId::from_legacy_or_new(id),
            provider_id: format!("provider-{id}"),
            name: "AskUserQuestion".to_string(),
            index: 0,
            input: serde_json::json!({"question": "test"}),
        }
    }

    fn dummy_suspension() -> tools::ToolSuspension {
        tools::ToolSuspension::UserInteraction(tools::UserInteractionSpec::new(vec![
            tools::UserQuestion {
                prompt: "test?".to_string(),
                options: vec![],
                allow_multi: false,
                allow_free_input: false,
                default: None,
            },
        ]))
    }

    fn auth() -> tools::AuthorizationContext {
        tools::AuthorizationContext::STANDARD
    }

    #[tokio::test]
    async fn test_empty_suspended_returns_empty() {
        let sink = RecordingSink::new();
        let hook_port = noop_hook_port();
        let cancel = tokio_util::sync::CancellationToken::new();
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));

        let results = ask_user(
            &context,
            &sink,
            &hook_port,
            &[],
            &cancel,
            std::path::Path::new("."),
        )
        .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_ask_user_handles_cancellation() {
        let sink = RecordingSink::new();
        let hook_port = noop_hook_port();
        let cancel = tokio_util::sync::CancellationToken::new();
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));

        let call = dummy_call("call-1");
        let suspension = dummy_suspension();
        let calls = vec![(&call, suspension, auth())];

        // Cancel before waiting for reply
        cancel.cancel();
        let results = ask_user(
            &context,
            &sink,
            &hook_port,
            &calls,
            &cancel,
            std::path::Path::new("."),
        )
        .await;

        assert_eq!(results.len(), 1);
        assert!(results[0].outcome.is_error);
        assert!(results[0].outcome.text.contains("Cancelled"));
    }

    #[tokio::test]
    async fn test_permission_request_hook_is_called() {
        let sink = RecordingSink::new();
        let hook_port = noop_hook_port();
        let cancel = tokio_util::sync::CancellationToken::new();
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));

        let call = dummy_call("call-1");
        let suspension = dummy_suspension();
        let calls = vec![(&call, suspension, auth())];

        // Cancel immediately to avoid waiting for reply
        cancel.cancel();
        let _ = ask_user(
            &context,
            &sink,
            &hook_port,
            &calls,
            &cancel,
            std::path::Path::new("."),
        )
        .await;
    }

    #[tokio::test]
    async fn test_permission_request_skipped_when_disabled() {
        let sink = RecordingSink::new();
        let hook_port = noop_hook_port();
        let cancel = tokio_util::sync::CancellationToken::new();
        let context = RuntimeTurnContext::new(ChatId::new("chat"), ChatTurnId::new("turn"));

        let call = dummy_call("call-1");
        let suspension = dummy_suspension();
        let no_hooks_auth = tools::AuthorizationContext {
            enforce_permission_hooks: false,
            ..tools::AuthorizationContext::STANDARD
        };
        let calls = vec![(&call, suspension, no_hooks_auth)];

        cancel.cancel();
        let _ = ask_user(
            &context,
            &sink,
            &hook_port,
            &calls,
            &cancel,
            std::path::Path::new("."),
        )
        .await;
    }
}

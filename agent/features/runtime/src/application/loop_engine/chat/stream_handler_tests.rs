use super::events::{ChatEventSink, RuntimeRunContext, RuntimeStreamEvent};
use super::stream_handler::InvocationEventReducer;
use crate::application::tool::coordination::identity::ToolIdentityRegistry;
use provider::{
    InvocationDelta, InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderErrorKind,
    ProviderStopReason, ProviderToolCall, ProviderToolCallId, ReasoningLevel,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<RuntimeStreamEvent>>>);

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> super::events::EventFuture<'a> {
        Box::pin(async move { self.0.lock().unwrap().push(event) })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn completion(output: Vec<ProviderContentBlock>) -> InvocationEvent {
    InvocationEvent::Completed(ProviderCompletion {
        output,
        stop_reason: ProviderStopReason::EndTurn,
        usage: None,
        effective_reasoning: ReasoningLevel::Off,
    })
}

#[test]
fn reducer_keeps_tool_identity_isolated_per_turn() {
    let sink = RecordingSink::default();
    let registry = ToolIdentityRegistry::new();
    let first_context =
        RuntimeRunContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatRunId::new_v7());
    let second_context =
        RuntimeRunContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatRunId::new_v7());
    let mut first =
        InvocationEventReducer::with_tool_identity(sink.clone(), registry.clone(), first_context);
    let mut second =
        InvocationEventReducer::with_tool_identity(sink.clone(), registry, second_context);

    first
        .apply(InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
            index: 0,
            provider_id: Some(ProviderToolCallId("provider-a".into())),
            name: "Read".into(),
        }))
        .unwrap();
    second
        .apply(InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
            index: 0,
            provider_id: Some(ProviderToolCallId("provider-b".into())),
            name: "Read".into(),
        }))
        .unwrap();

    let ids: Vec<_> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            RuntimeStreamEvent::ToolCallStart { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn reducer_rejects_empty_terminal_completions_as_retryable_protocol_errors() {
    let cases = [
        ("empty output", Vec::new()),
        (
            "empty text",
            vec![ProviderContentBlock::Text(String::new())],
        ),
        (
            "whitespace text",
            vec![ProviderContentBlock::Text("   \n".into())],
        ),
        (
            "thinking only",
            vec![ProviderContentBlock::Thinking {
                thinking: "internal reasoning".into(),
                signature: None,
            }],
        ),
    ];

    for (label, output) in cases {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        let error = reducer.apply(completion(output)).expect_err(label);
        assert_eq!(error.kind, ProviderErrorKind::Protocol, "{label}");
        assert!(error.retryable, "{label}");
        assert!(
            error.safe_message.contains("assistant text or tool call"),
            "{label}: {}",
            error.safe_message
        );
    }
}

#[test]
fn reducer_accepts_nonblank_text_and_tool_call_terminal_completions() {
    let cases = [
        vec![ProviderContentBlock::Text("answer".into())],
        vec![ProviderContentBlock::ToolCall(ProviderToolCall {
            id: ProviderToolCallId("tool-1".into()),
            name: "Read".into(),
            arguments: serde_json::json!({}),
        })],
    ];

    for output in cases {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        let response = reducer.apply(completion(output)).unwrap().unwrap();
        assert_eq!(
            response.assistant_message.role,
            share::message::Role::Assistant
        );
    }
}

#[test]
fn reducer_projects_block_transitions_without_callback_contract() {
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink.clone());
    reducer
        .apply(InvocationEvent::Delta(InvocationDelta::Thinking {
            thinking: "thought".into(),
            signature: None,
        }))
        .unwrap();
    reducer
        .apply(InvocationEvent::Delta(InvocationDelta::Text(
            "answer".into(),
        )))
        .unwrap();
    reducer
        .apply(completion(vec![ProviderContentBlock::Text(
            "answer".into(),
        )]))
        .unwrap();

    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(
        |event| matches!(event, RuntimeStreamEvent::Thinking { text, .. } if text == "thought")
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::Text { text, .. } if text == "answer")));
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::BlockComplete { .. })));
}

#[test]
fn reducer_closes_active_block_for_synthetic_raw_eof_failure() {
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink.clone());
    reducer
        .apply(InvocationEvent::Delta(InvocationDelta::Text(
            "partial".into(),
        )))
        .unwrap();

    let error = provider::ProviderError::retryable(
        ProviderErrorKind::StreamTruncated,
        "provider stream ended without terminal event",
    );
    let returned = reducer
        .apply(InvocationEvent::Failed(error.clone()))
        .expect_err("failure event should terminate the invocation");
    assert_eq!(returned.kind, ProviderErrorKind::StreamTruncated);

    let events = sink.0.lock().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::BlockComplete { .. })));
}

use super::events::{ChatEventSink, RuntimeRunContext, RuntimeStreamEvent};
use super::stream_handler::{InvocationEventReducer, InvocationResponse};
use crate::application::tool::coordination::identity::ToolIdentityRegistry;
use provider::{
    ProviderContentData, ProviderErrorKind, ProviderResponseChunk, ProviderStopReasonData,
    ProviderToolCallData,
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

/// v3 拆帧：原 `Completed{output}` → 每块一个 `Content` 帧 + `Stop` 终止帧
/// （`usage: None` 等价于不发 `Usage` 帧——两者皆聚合为 default）。
fn completion(output: Vec<ProviderContentData>) -> Vec<ProviderResponseChunk> {
    let mut chunks: Vec<_> = output
        .into_iter()
        .map(ProviderResponseChunk::Content)
        .collect();
    chunks.push(ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn));
    chunks
}

/// 依序应用一整段 completion 帧序列，返回末帧（`Stop`）结果。
fn apply_completion(
    reducer: &mut InvocationEventReducer<RecordingSink>,
    output: Vec<ProviderContentData>,
) -> Result<Option<InvocationResponse>, provider::ProviderError> {
    let mut last = Ok(None);
    for chunk in completion(output) {
        last = reducer.apply(chunk);
    }
    last
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
        .apply(ProviderResponseChunk::Content(
            ProviderContentData::ToolCallStarted {
                index: 0,
                provider_id: Some("provider-a".into()),
                name: "Read".into(),
            },
        ))
        .unwrap();
    second
        .apply(ProviderResponseChunk::Content(
            ProviderContentData::ToolCallStarted {
                index: 0,
                provider_id: Some("provider-b".into()),
                name: "Read".into(),
            },
        ))
        .unwrap();

    let ids: Vec<_> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            RuntimeStreamEvent::ToolCallStarted { id, .. } => Some(id.clone()),
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
        ("empty text", vec![ProviderContentData::Text(String::new())]),
        (
            "whitespace text",
            vec![ProviderContentData::Text("   \n".into())],
        ),
        (
            "thinking only",
            vec![ProviderContentData::Thinking {
                thinking: "internal reasoning".into(),
                signature: None,
            }],
        ),
    ];

    for (label, output) in cases {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        let error = apply_completion(&mut reducer, output).expect_err(label);
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
        vec![ProviderContentData::Text("answer".into())],
        vec![ProviderContentData::ToolCall(ProviderToolCallData {
            id: "tool-1".into(),
            name: "Read".into(),
            arguments: serde_json::json!({}),
        })],
    ];

    for output in cases {
        let mut reducer = InvocationEventReducer::new(RecordingSink::default());
        let response = apply_completion(&mut reducer, output).unwrap().unwrap();
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
        .apply(ProviderResponseChunk::Content(
            ProviderContentData::Thinking {
                thinking: "thought".into(),
                signature: None,
            },
        ))
        .unwrap();
    reducer
        .apply(ProviderResponseChunk::Content(ProviderContentData::Text(
            "answer".into(),
        )))
        .unwrap();
    reducer
        .apply(ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn))
        .unwrap();

    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(
        |event| matches!(event, RuntimeStreamEvent::ThinkingDelta { delta, .. } if delta == "thought")
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::AssistantTextDelta { delta, .. } if delta == "answer")));
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::BlockComplete { .. })));
}

#[test]
fn reducer_closes_active_block_for_synthetic_raw_eof_failure() {
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink.clone());
    reducer
        .apply(ProviderResponseChunk::Content(ProviderContentData::Text(
            "partial".into(),
        )))
        .unwrap();

    let error = provider::ProviderError::retryable(
        ProviderErrorKind::StreamTruncated,
        "provider stream ended without terminal event",
    );
    let returned = reducer
        .apply(ProviderResponseChunk::Error(error.clone()))
        .expect_err("failure event should terminate the invocation");
    assert_eq!(returned.kind, ProviderErrorKind::StreamTruncated);

    let events = sink.0.lock().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, RuntimeStreamEvent::BlockComplete { .. })));
}

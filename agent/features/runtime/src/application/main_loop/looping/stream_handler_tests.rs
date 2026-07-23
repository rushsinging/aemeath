use super::events::{ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext};
use super::stream_handler::{should_emit_model_stream_waiting, InvocationEventReducer};
use crate::application::tool_coordination::identity::ToolIdentityRegistry;
use provider::{
    InvocationDelta, InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderStopReason,
    ProviderToolCall, ProviderToolCallId, ReasoningLevel,
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
        RuntimeTurnContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatTurnId::new_v7());
    let second_context =
        RuntimeTurnContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatTurnId::new_v7());
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
fn reducer_progress_tracks_visible_deltas_and_waiting_phase() {
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink);
    let progress = reducer.progress_handle();
    let initial = progress.lock().unwrap().snapshot();
    assert_eq!(initial.phase, "waiting_model_response");

    reducer
        .apply(InvocationEvent::Delta(InvocationDelta::Thinking {
            thinking: "thinking".into(),
            signature: None,
        }))
        .unwrap();
    let thinking = progress.lock().unwrap().snapshot();
    assert_eq!(thinking.phase, "thinking");
    assert!(thinking.first_visible_event_seen);

    reducer
        .apply(completion(vec![ProviderContentBlock::Thinking {
            thinking: "thinking".into(),
            signature: None,
        }]))
        .unwrap();
    let waiting = progress.lock().unwrap().snapshot();
    assert_eq!(waiting.phase, "waiting_model_output");
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
fn waiting_detection_requires_no_progress_since_previous_snapshot() {
    let sink = RecordingSink::default();
    let mut reducer = InvocationEventReducer::new(sink);
    let progress = reducer.progress_handle();
    let initial = progress.lock().unwrap().snapshot();
    assert!(should_emit_model_stream_waiting(None, &initial));
    reducer
        .apply(InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
            index: 0,
            call: ProviderToolCall {
                id: ProviderToolCallId("provider-tool".into()),
                name: "Read".into(),
                arguments: serde_json::json!({}),
            },
        }))
        .unwrap();
    let unchanged = progress.lock().unwrap().snapshot();
    assert!(should_emit_model_stream_waiting(
        Some(initial.visible_progress_version),
        &unchanged
    ));
}

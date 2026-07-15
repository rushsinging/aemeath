use super::stream_handler::{should_emit_model_stream_waiting, RuntimeStreamHandler};
use super::tool_identity::ToolIdentityRegistry;
use super::{ChatEventSink, EventFuture, RuntimeStreamEvent, RuntimeTurnContext};
use provider::api::StreamHandler;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[test]
fn test_stream_handler_keeps_runtime_tool_ids_unique_across_handlers() {
    let sink = RecordingSink::default();
    let registry = ToolIdentityRegistry::new();
    let chat_id = sdk::ids::ChatId::new_v7();
    let turn_id = sdk::ids::ChatTurnId::new_v7();
    let mut first = RuntimeStreamHandler::with_tool_identity(
        sink.clone(),
        registry.clone(),
        RuntimeTurnContext::new(chat_id.clone(), turn_id.clone()),
    );
    let mut second = RuntimeStreamHandler::with_tool_identity(
        sink.clone(),
        registry,
        RuntimeTurnContext::new(chat_id, turn_id),
    );

    first.on_tool_use_start("Read", Some("provider-a"), 0);
    second.on_tool_use_start("Read", Some("provider-b"), 0);

    let events = sink.events.lock().unwrap();
    let ids: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            RuntimeStreamEvent::ToolCallStart { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "runtime tool id 必须 session 级唯一");
}

#[test]
fn test_stream_handler_progress_snapshot_tracks_waiting_phases() {
    let sink = RecordingSink::default();
    let mut handler = RuntimeStreamHandler::new(sink.clone());

    let initial = handler.progress_snapshot();
    assert_eq!(initial.phase, "waiting_model_response");
    assert_eq!(initial.visible_progress_version, 0);
    handler.on_thinking("thinking");
    let thinking = handler.progress_snapshot();
    assert_eq!(thinking.phase, "thinking");
    assert_eq!(thinking.visible_progress_version, 1);
    handler.on_tool_use_start("Write", Some("provider-tool"), 0);
    let waiting = handler.progress_snapshot();
    assert_eq!(waiting.phase, "waiting_model_output");
    assert_eq!(waiting.visible_progress_version, 2);
}

#[test]
fn test_stream_handler_progress_version_advances_for_continuous_thinking() {
    let sink = RecordingSink::default();
    let mut handler = RuntimeStreamHandler::new(sink.clone());

    handler.on_thinking("first");
    let first = handler.progress_snapshot();
    handler.on_thinking("second");
    let second = handler.progress_snapshot();

    assert_eq!(first.phase, "thinking");
    assert_eq!(second.phase, "thinking");
    assert!(
        second.visible_progress_version > first.visible_progress_version,
        "持续 thinking delta 必须刷新可见进展版本，避免 idle watcher 误发等待占位"
    );
    assert!(should_emit_model_stream_waiting(
        Some(first.visible_progress_version),
        &first
    ));
    assert!(
        !should_emit_model_stream_waiting(Some(first.visible_progress_version), &second),
        "两次 watcher 检查之间出现新的 thinking delta 时，不应发送 ModelStreamWaiting 占位"
    );
}

#[test]
fn test_stream_handler_emits_block_complete_between_thinking_and_tool_call() {
    let sink = RecordingSink::default();
    let mut handler = RuntimeStreamHandler::new(sink.clone());
    handler.on_thinking("first thought");
    handler.on_tool_use_start("Read", Some("provider-tool"), 0);
    handler.on_thinking("second thought");

    let events = sink.events.lock().unwrap();
    let event_kinds: Vec<_> = events
        .iter()
        .map(|event| match event {
            RuntimeStreamEvent::Thinking { .. } => "thinking",
            RuntimeStreamEvent::ToolCallStart { .. } => "tool_start",
            RuntimeStreamEvent::BlockComplete { .. } => "complete",
            _ => "other",
        })
        .collect();

    assert_eq!(
        event_kinds,
        vec!["thinking", "complete", "tool_start", "thinking"]
    );
}

#[test]
fn test_stream_handler_emits_block_complete_when_text_kind_changes() {
    let sink = RecordingSink::default();
    let mut handler = RuntimeStreamHandler::new(sink.clone());

    handler.on_thinking("thought");
    handler.on_text("answer");

    let events = sink.events.lock().unwrap();
    let event_kinds: Vec<_> = events
        .iter()
        .map(|event| match event {
            RuntimeStreamEvent::Thinking { .. } => "thinking",
            RuntimeStreamEvent::Text { .. } => "text",
            RuntimeStreamEvent::BlockComplete { .. } => "complete",
            _ => "other",
        })
        .collect();

    assert_eq!(event_kinds, vec!["thinking", "complete", "text"]);
}

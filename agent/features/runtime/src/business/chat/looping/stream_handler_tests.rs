use super::stream_handler::RuntimeStreamHandler;
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
    let mut first = RuntimeStreamHandler::with_tool_identity(
        sink.clone(),
        registry.clone(),
        RuntimeTurnContext::new("chat", "turn-1"),
    );
    let mut second = RuntimeStreamHandler::with_tool_identity(
        sink.clone(),
        registry,
        RuntimeTurnContext::new("chat", "turn-1"),
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
fn test_stream_handler_forwards_provider_id_on_start_and_delta() {
    let sink = RecordingSink::default();
    let mut handler = RuntimeStreamHandler::new(sink.clone());

    handler.on_tool_use_start("Skill", Some("call-provider-skill"), 0);
    handler.on_tool_arguments_delta(0, "Skill", Some("call-provider-skill"), r#"{"skill""#);

    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeStreamEvent::ToolCallStart { provider_id, .. }
            if provider_id.as_deref() == Some("call-provider-skill")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeStreamEvent::ToolArgumentsDelta { provider_id, .. }
            if provider_id.as_deref() == Some("call-provider-skill")
    )));
}

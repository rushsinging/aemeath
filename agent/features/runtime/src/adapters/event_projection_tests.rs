use super::event_projection::project_stream_event;
use crate::application::chat::looping::{
    RuntimeHookMessage, RuntimeHookMessageKind, RuntimeStreamEvent, RuntimeTurnContext,
};

#[test]
fn tool_call_projection_preserves_canonical_name() {
    let event = RuntimeStreamEvent::ToolCallStart {
        context: RuntimeTurnContext::new(
            sdk::ids::ChatId::new("chat-1"),
            sdk::ids::ChatTurnId::new("turn-1"),
        ),
        id: sdk::ids::ToolCallId::new("tool-1"),
        provider_id: Some("provider-1".to_string()),
        name: "Grep".to_string(),
        index: 0,
    };

    match project_stream_event(event) {
        sdk::ChatEvent::ToolCallStart { name, .. } => assert_eq!(name, "Grep"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn hook_message_projection_preserves_additional_context_attribution() {
    let event = RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
        point: hook::HookPoint::PreToolUse,
        source: "Bash".to_string(),
        execution_ordinal: 0,
        attempt: 1,
        kind: RuntimeHookMessageKind::AdditionalContext,
        text: "extra context".to_string(),
    });

    match project_stream_event(event) {
        sdk::ChatEvent::HookMessage(view) => {
            assert_eq!(view.point, "PreToolUse");
            assert_eq!(view.source, "Bash");
            assert_eq!(view.execution_ordinal, 0);
            assert_eq!(view.attempt, 1);
            assert_eq!(view.kind, sdk::HookMessageKindView::AdditionalContext);
            assert_eq!(view.text, "extra context");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn hook_message_projection_preserves_system_message_attempt() {
    let event = RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
        point: hook::HookPoint::PostToolUse,
        source: "Bash".to_string(),
        execution_ordinal: 2,
        attempt: 3,
        kind: RuntimeHookMessageKind::SystemMessage,
        text: "warning".to_string(),
    });

    match project_stream_event(event) {
        sdk::ChatEvent::HookMessage(view) => {
            assert_eq!(view.kind, sdk::HookMessageKindView::SystemMessage);
            assert_eq!(view.execution_ordinal, 2);
            assert_eq!(view.attempt, 3);
            assert_eq!(view.text, "warning");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

use super::sdk_event_mapper::map_stream_event;
use crate::application::loop_engine::chat::{
    RuntimeHookMessage, RuntimeHookMessageKind, RuntimeResumedSessionStep, RuntimeStreamEvent,
    RuntimeTurnContext,
};

#[test]
fn session_resume_mapping_preserves_context_run_step_boundaries() {
    let event = RuntimeStreamEvent::SessionResumed {
        steps: vec![RuntimeResumedSessionStep {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            messages: vec![share::message::Message::user("hello")],
            finalize_cause: None,
            duration_ms: None,
        }],
        session_id: "session-1".into(),
        created_at: 0,
    };

    match map_stream_event(event) {
        sdk::ChatEvent::SessionResumed { steps, .. } => {
            assert_eq!(steps[0].run_id, "run-1");
            assert_eq!(steps[0].step_id, "step-1");
            assert_eq!(steps[0].messages[0].text_content(), "hello");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn tool_call_mapping_preserves_canonical_name() {
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

    match map_stream_event(event) {
        sdk::ChatEvent::ToolCallStart { name, .. } => assert_eq!(name, "Grep"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn hook_message_mapping_preserves_additional_context_attribution() {
    let event = RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
        point: hook::HookPoint::PreToolUse,
        source: "Bash".to_string(),
        execution_ordinal: 0,
        attempt: 1,
        kind: RuntimeHookMessageKind::AdditionalContext,
        text: "extra context".to_string(),
    });

    match map_stream_event(event) {
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
fn hook_message_mapping_preserves_system_message_attempt() {
    let event = RuntimeStreamEvent::HookMessage(RuntimeHookMessage {
        point: hook::HookPoint::PostToolUse,
        source: "Bash".to_string(),
        execution_ordinal: 2,
        attempt: 3,
        kind: RuntimeHookMessageKind::SystemMessage,
        text: "warning".to_string(),
    });

    match map_stream_event(event) {
        sdk::ChatEvent::HookMessage(view) => {
            assert_eq!(view.kind, sdk::HookMessageKindView::SystemMessage);
            assert_eq!(view.execution_ordinal, 2);
            assert_eq!(view.attempt, 3);
            assert_eq!(view.text, "warning");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn config_reload_mapping_preserves_immediate_scope_and_committed_view() {
    let event = RuntimeStreamEvent::ConfigReloaded {
        changed_keys: vec![
            "config:reloaded".to_string(),
            "config:scope:immediate".to_string(),
        ],
        view: sdk::ConfigView {
            markdown_spacing: sdk::MarkdownSpacingModeView::Compact,
            ..Default::default()
        },
    };

    match map_stream_event(event) {
        sdk::ChatEvent::ConfigReloaded { event } => {
            assert_eq!(
                event.scopes,
                vec![sdk::ConfigApplicationScopeView::Immediate]
            );
            assert_eq!(
                event.view.markdown_spacing,
                sdk::MarkdownSpacingModeView::Compact
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn model_invocation_retry_mapping_preserves_context_attempt_and_delay() {
    let context = RuntimeTurnContext::new(
        sdk::ids::ChatId::new("chat-retry"),
        sdk::ids::ChatTurnId::new("turn-retry"),
    );
    let expected_chat_id = context.chat_id.clone();
    let expected_turn_id = context.turn_id.clone();
    let event = RuntimeStreamEvent::ModelInvocationRetrying {
        context,
        attempt: 2,
        delay: std::time::Duration::from_millis(10_250),
    };

    match map_stream_event(event) {
        sdk::ChatEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => {
            assert_eq!(context.chat_id, expected_chat_id);
            assert_eq!(context.turn_id, expected_turn_id);
            assert_eq!(attempt, 2);
            assert_eq!(delay, std::time::Duration::from_millis(10_250));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

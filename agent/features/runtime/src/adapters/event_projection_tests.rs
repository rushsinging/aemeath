use super::event_projection::project_stream_event;
use crate::application::main_loop::looping::{
    RuntimeHookMessage, RuntimeHookMessageKind, RuntimeResumedSessionStep, RuntimeStreamEvent,
    RuntimeTurnContext,
};

#[test]
fn cancelled_projection_preserves_elapsed_duration() {
    let event = RuntimeStreamEvent::Cancelled {
        context: RuntimeTurnContext::new(sdk::ChatId::new("chat"), sdk::ChatTurnId::new("turn")),
        duration: std::time::Duration::from_millis(125_000),
    };

    assert!(matches!(
        project_stream_event(event),
        sdk::ChatEvent::Cancelled {
            duration_ms: 125_000,
            ..
        }
    ));
}

#[test]
fn session_resume_projection_preserves_context_run_step_boundaries() {
    let event = RuntimeStreamEvent::SessionResumed {
        steps: vec![RuntimeResumedSessionStep {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            messages: vec![share::message::Message::user("hello")],
            finalize_cause: Some(context::domain::FinalizeCause::UserCancelledStep),
            duration_ms: Some(7_325_000),
        }],
        session_id: "session-1".into(),
        created_at: 0,
    };

    match project_stream_event(event) {
        sdk::ChatEvent::SessionResumed { steps, .. } => {
            assert_eq!(steps[0].run_id, "run-1");
            assert_eq!(steps[0].step_id, "step-1");
            assert_eq!(steps[0].messages[0].text_content(), "hello");
            assert_eq!(
                steps[0].finalize_cause,
                Some(sdk::ResumedStepFinalizeCause::UserCancelledStep)
            );
            assert_eq!(steps[0].duration_ms, Some(7_325_000));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn session_resume_projection_preserves_reconstructed_tool_pair_and_termination() {
    let event = RuntimeStreamEvent::SessionResumed {
        steps: vec![RuntimeResumedSessionStep {
            run_id: "run-terminated".into(),
            step_id: "step-running-tool".into(),
            messages: vec![
                share::message::Message {
                    role: share::message::Role::Assistant,
                    content: vec![share::message::ContentBlock::ToolUse {
                        id: "provider-call-1".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({"command": "sleep 180"}),
                    }],
                    metadata: None,
                },
                share::message::Message {
                    role: share::message::Role::User,
                    content: vec![share::message::ContentBlock::ToolResult {
                        tool_use_id: "provider-call-1".into(),
                        content: serde_json::json!({"outcome": "CancellationUnconfirmed"}),
                        is_error: true,
                        text: Some("cleanup could not be confirmed".into()),
                    }],
                    metadata: None,
                },
            ],
            finalize_cause: Some(context::domain::FinalizeCause::RunTerminated),
            duration_ms: None,
        }],
        session_id: "session-1".into(),
        created_at: 0,
    };

    let sdk::ChatEvent::SessionResumed { steps, .. } = project_stream_event(event) else {
        panic!("应投影为 SessionResumed");
    };
    assert_eq!(
        steps[0].finalize_cause,
        Some(sdk::ResumedStepFinalizeCause::RunTerminated)
    );
    assert!(matches!(
        &steps[0].messages[0].content[0],
        sdk::ContentBlock::ToolUse { id, name, .. }
            if id == "provider-call-1" && name == "Bash"
    ));
    assert!(matches!(
        &steps[0].messages[1].content[0],
        sdk::ContentBlock::ToolResult { tool_use_id, is_error: true, .. }
            if tool_use_id == "provider-call-1"
    ));
}

#[test]
fn tool_result_projection_preserves_bounded_content_without_reconstruction() {
    let content = serde_json::json!({
        "text": "bounded preview",
        "truncated": true,
        "original_chars": 50_001,
        "original_bytes": 50_001,
        "omitted_chars": 47_501,
        "blob": {
            "status": "unavailable",
            "reason": "write_failed"
        }
    });
    let event = RuntimeStreamEvent::ToolResult {
        context: RuntimeTurnContext::new(
            sdk::ids::ChatId::new("chat-tool-result"),
            sdk::ids::ChatTurnId::new("turn-tool-result"),
        ),
        id: sdk::ids::ToolCallId::new("runtime-call"),
        provider_id: "provider-call".to_string(),
        tool_name: "Bash".to_string(),
        output: "bounded preview".to_string(),
        content: content.clone(),
        is_error: false,
        images: Vec::new(),
    };

    let sdk::ChatEvent::ToolResult {
        output,
        content: projected,
        ..
    } = project_stream_event(event)
    else {
        panic!("expected SDK tool result");
    };

    assert_eq!(output, "bounded preview");
    assert_eq!(projected, content);
    assert_eq!(
        projected
            .pointer("/blob/reason")
            .and_then(serde_json::Value::as_str),
        Some("write_failed")
    );
}

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

#[test]
fn config_reload_projection_preserves_immediate_scope_and_committed_view() {
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

    match project_stream_event(event) {
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
fn model_invocation_retry_projection_preserves_context_attempt_and_delay() {
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

    match project_stream_event(event) {
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

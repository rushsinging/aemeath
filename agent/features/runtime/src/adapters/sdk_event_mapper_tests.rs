use super::sdk_event_mapper::map_stream_event;
use crate::application::loop_engine::chat::events::RuntimeHookExecutionResult;
use crate::application::loop_engine::chat::{
    RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookMessage, RuntimeHookMessageKind,
    RuntimeResumedSessionStep, RuntimeStreamEvent, RuntimeTurnContext,
};

#[test]
fn activity_events_map_without_losing_change_or_snapshot_facts() {
    let activity = sdk::ActivityView {
        id: sdk::ActivityId::new("activity-map"),
        run_id: sdk::RunId::new("run-map"),
        run_step_id: None,
        parent_activity_id: None,
        source: sdk::ActivitySourceView::Run,
        kind: sdk::ActivityKindView::Run,
        state: sdk::ActivityStateView::Running,
        detail: sdk::ActivityDetailView::Run {
            purpose: sdk::RunPurposeView::Main,
        },
        audience: sdk::ActivityAudienceView::User,
        revision: 3,
        timing: sdk::ActivityTimingView::default(),
    };

    let changed = map_stream_event(RuntimeStreamEvent::ActivityChanged {
        kind: sdk::ActivityChangeKind::Updated,
        activity: activity.clone(),
    });
    let snapshot = map_stream_event(RuntimeStreamEvent::ActivitySnapshot(
        sdk::ActivitySnapshotView {
            run_id: activity.run_id.clone(),
            revision: 3,
            activities: vec![activity.clone()],
        },
    ));

    assert!(matches!(
        changed,
        sdk::ChatEvent::ActivityChanged {
            kind: sdk::ActivityChangeKind::Updated,
            activity: mapped,
        } if mapped == activity
    ));
    assert!(matches!(
        snapshot,
        sdk::ChatEvent::ActivitySnapshot(mapped)
            if mapped.revision == 3 && mapped.activities == vec![activity]
    ));
}

#[test]
fn sdk_agent_progress_preserves_source_and_attachment_contexts() {
    let source_context = RuntimeTurnContext::new(
        sdk::ids::ChatId::new("child-chat"),
        sdk::ids::ChatTurnId::new("child-turn"),
    );
    let attachment_context = RuntimeTurnContext::new(
        sdk::ids::ChatId::new("parent-chat"),
        sdk::ids::ChatTurnId::new("parent-turn"),
    );
    let expected_source = source_context.clone();
    let expected_attachment = attachment_context.clone();
    let tool_id = sdk::ids::ToolCallId::new("agent-tool");
    let event = RuntimeStreamEvent::AgentProgress {
        source_context,
        attachment_context,
        tool_id: tool_id.clone(),
        event: tools::AgentProgressEvent {
            source_context: None,
            sequence: 7,
            kind: tools::AgentProgressKind::Message {
                text: "working".to_string(),
            },
        },
    };

    match map_stream_event(event) {
        sdk::ChatEvent::AgentProgress {
            source_context,
            attachment_context,
            tool_id: mapped_tool_id,
            event,
        } => {
            assert_eq!(source_context.chat_id, expected_source.chat_id);
            assert_eq!(source_context.turn_id, expected_source.turn_id);
            assert_eq!(attachment_context.chat_id, expected_attachment.chat_id);
            assert_eq!(attachment_context.turn_id, expected_attachment.turn_id);
            assert_eq!(mapped_tool_id, tool_id);
            assert_eq!(event.sequence, 7);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

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
    } = map_stream_event(event)
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

    match map_stream_event(event) {
        sdk::ChatEvent::ToolCallStart { name, .. } => assert_eq!(name, "Grep"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn hook_event_mapping_preserves_authoritative_status_and_final_diagnostics() {
    let event = RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
        hook_name: "Stop".to_string(),
        status: RuntimeHookEventStatus::Succeeded,
        matcher: Some("*".to_string()),
        command: Some("check-agent-stop.sh".to_string()),
        result: Some(RuntimeHookExecutionResult {
            exit_code: Some(0),
            stdout: "ok".to_string(),
            stderr: String::new(),
            decision: Some("continue".to_string()),
            reason: None,
            additional_context: None,
        }),
    });

    match map_stream_event(event) {
        sdk::ChatEvent::HookEvent(view) => {
            assert_eq!(view.status, sdk::HookEventStatus::Succeeded);
            assert_eq!(view.matcher.as_deref(), Some("*"));
            assert_eq!(view.command.as_deref(), Some("check-agent-stop.sh"));
            let result = view.result.expect("hook result");
            assert_eq!(result.exit_code, Some(0));
            assert_eq!(result.stdout, "ok");
            assert!(result.stderr.is_empty());
            assert_eq!(result.decision.as_deref(), Some("continue"));
            assert!(result.reason.is_none());
        }
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

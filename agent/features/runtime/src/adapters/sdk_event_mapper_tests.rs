use super::sdk_event_mapper::map_stream_event;
use crate::application::loop_engine::chat::{
    RuntimeResumedSessionStep, RuntimeRunContext, RuntimeStreamEvent,
};
#[test]
fn adopted_input_mapping_preserves_input_ids_and_order_for_sdk() {
    let first_id = sdk::InputId::new("input-a");
    let second_id = sdk::InputId::new("input-b");
    let queued_id = sdk::InputId::new("input-c");
    let event = RuntimeStreamEvent::UserMessagesAdopted {
        items: vec![
            (first_id.clone(), share::message::Message::user("first")),
            (second_id.clone(), share::message::Message::user("second")),
        ],
        queued: vec![(queued_id.clone(), share::message::Message::user("queued"))],
    };

    match map_stream_event(event) {
        sdk::ChatEvent::UserMessagesAdopted { items, queued } => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].input_id.as_ref(), Some(&first_id));
            assert_eq!(items[0].text_content(), "first");
            assert_eq!(items[1].input_id.as_ref(), Some(&second_id));
            assert_eq!(items[1].text_content(), "second");
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].input_id.as_ref(), Some(&queued_id));
            assert_eq!(queued[0].text_content(), "queued");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn adopted_typed_skill_request_mapping_preserves_display_metadata_for_sdk() {
    let input_id = sdk::InputId::new("skill-input");
    let event = RuntimeStreamEvent::UserMessagesAdopted {
        items: vec![(
            input_id.clone(),
            share::message::Message::skill_request(
                "LLM prompt",
                share::message::SkillRequestMetadata {
                    skill: "superpowers:brainstorming".to_string(),
                    arguments: "feature scope".to_string(),
                    raw_input: "/superpowers:brainstorming feature scope".to_string(),
                },
            ),
        )],
        queued: Vec::new(),
    };

    match map_stream_event(event) {
        sdk::ChatEvent::UserMessagesAdopted { items, .. } => {
            assert_eq!(items[0].input_id.as_ref(), Some(&input_id));
            assert_eq!(
                items[0].metadata.as_ref().map(|metadata| metadata.source),
                Some(sdk::ChatMessageSource::SkillRequest)
            );
            assert_eq!(
                items[0]
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.skill_request.as_ref())
                    .map(|request| request.raw_input.as_str()),
                Some("/superpowers:brainstorming feature scope")
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

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
    let source_context = RuntimeRunContext::new(
        sdk::ids::ChatId::new("child-chat"),
        sdk::ids::ChatRunId::new("child-turn"),
    );
    let attachment_context = RuntimeRunContext::new(
        sdk::ids::ChatId::new("parent-chat"),
        sdk::ids::ChatRunId::new("parent-turn"),
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
            assert_eq!(source_context.run_id, expected_source.run_id);
            assert_eq!(attachment_context.chat_id, expected_attachment.chat_id);
            assert_eq!(attachment_context.run_id, expected_attachment.run_id);
            assert_eq!(mapped_tool_id, tool_id);
            assert_eq!(event.sequence, 7);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn sdk_tool_progress_preserves_context_tool_id_and_text() {
    let context = RuntimeRunContext::new(
        sdk::ids::ChatId::new("chat-1"),
        sdk::ids::ChatRunId::new("run-1"),
    );
    let expected_context = context.clone();
    let tool_id = sdk::ids::ToolCallId::new("bash-tool");
    let event = RuntimeStreamEvent::ToolProgress {
        context,
        tool_id: tool_id.clone(),
        event: tools::ToolProgressEvent {
            text: "checking PRs…\n".to_string(),
        },
    };

    match map_stream_event(event) {
        sdk::ChatEvent::ToolProgress {
            context,
            tool_id: mapped_tool_id,
            event,
        } => {
            assert_eq!(context.chat_id, expected_context.chat_id);
            assert_eq!(context.run_id, expected_context.run_id);
            assert_eq!(mapped_tool_id, tool_id);
            assert_eq!(event.text, "checking PRs…\n");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn session_resume_mapping_preserves_body_free_history_index() {
    let event = RuntimeStreamEvent::SessionResumed {
        steps: Vec::new(),
        display_history: Some(context::api::DisplayHistoryStepIndex::fixture(
            "session-index",
            17,
            vec![("run-1", "step-1", "step-run-step.json", 23)],
        )),
        session_id: "session-index".into(),
        created_at: 42,
        compacted: false,
    };

    match map_stream_event(event) {
        sdk::ChatEvent::SessionResumed {
            steps,
            display_history: Some(index),
            ..
        } => {
            assert!(steps.is_empty());
            assert_eq!(index.session_id, "session-index");
            assert_eq!(index.generation_revision, 17);
            assert_eq!(index.steps[0].member_name, "step-run-step.json");
            assert_eq!(index.steps[0].estimated_lines, 23);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn message_state_mapping_preserves_count_and_revision_without_snapshot() {
    match map_stream_event(RuntimeStreamEvent::SessionMessageStateChanged {
        message_count: 7,
        revision: 3,
    }) {
        sdk::ChatEvent::SessionMessageStateChanged {
            message_count,
            revision,
        } => {
            assert_eq!(message_count, 7);
            assert_eq!(revision, 3);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn hook_notice_mapping_preserves_point_kind_and_all_fields_for_sdk() {
    let notice = share::message::HookNotice {
        point: "PreToolUse".to_string(),
        kind: share::message::HookNoticeKind::Blocked,
        summary: "Stop hook 阻止了停止。".to_string(),
        command: "check-agent-stop.sh".to_string(),
        exit_code: Some(2),
        reason: "exit code 2".to_string(),
        stdout_preview: "stdout preview".to_string(),
        stderr_preview: "stderr preview".to_string(),
        stdout_truncated: true,
        stderr_truncated: false,
        output_file: Some("/tmp/stop-hook.txt".to_string()),
    };

    match map_stream_event(RuntimeStreamEvent::HookNotice(notice)) {
        sdk::ChatEvent::HookNotice { notice } => {
            assert_eq!(notice.point, "PreToolUse");
            assert_eq!(notice.kind, sdk::HookNoticeKindView::Blocked);
            assert_eq!(notice.summary, "Stop hook 阻止了停止。");
            assert_eq!(notice.command, "check-agent-stop.sh");
            assert_eq!(notice.exit_code, Some(2));
            assert_eq!(notice.reason, "exit code 2");
            assert_eq!(notice.stdout_preview, "stdout preview");
            assert_eq!(notice.stderr_preview, "stderr preview");
            assert!(notice.stdout_truncated);
            assert!(!notice.stderr_truncated);
            assert_eq!(notice.output_file.as_deref(), Some("/tmp/stop-hook.txt"));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn compact_finished_preserves_runtime_owned_notice() {
    let mapped = map_stream_event(RuntimeStreamEvent::CompactFinished {
        messages: vec![share::message::Message::user("recent")],
        notice: "✓ 上下文压缩完成".to_string(),
    });

    assert!(matches!(
        mapped,
        sdk::ChatEvent::CompactFinished { messages, notice }
            if messages.len() == 1
                && messages[0].text_content() == "recent"
                && notice == "✓ 上下文压缩完成"
    ));
}

#[test]
fn session_resume_mapping_preserves_context_run_step_boundaries_and_terminal_facts() {
    for finalize_cause in [
        context::domain::FinalizeCause::Completed,
        context::domain::FinalizeCause::UserCancelledStep,
        context::domain::FinalizeCause::RunTerminated,
    ] {
        let event = RuntimeStreamEvent::SessionResumed {
            steps: vec![RuntimeResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                message_segments: vec![vec![share::message::Message::user("hello")].into()],
                finalize_cause: Some(finalize_cause),
                duration_ms: Some(125_000),
            }],
            display_history: None,
            session_id: "session-1".into(),
            created_at: 0,
            compacted: false,
        };

        let expected_cause = crate::application::client::map_finalize_cause_to_sdk(finalize_cause);
        match map_stream_event(event) {
            sdk::ChatEvent::SessionResumed { steps, .. } => {
                assert_eq!(steps[0].run_id, "run-1");
                assert_eq!(steps[0].step_id, "step-1");
                assert_eq!(steps[0].messages[0].text_content(), "hello");
                assert_eq!(steps[0].finalize_cause, Some(expected_cause));
                assert_eq!(steps[0].duration_ms, Some(125_000));
            }
            other => panic!("unexpected event: {other:?}"),
        }
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
        context: RuntimeRunContext::new(
            sdk::ids::ChatId::new("chat-tool-result"),
            sdk::ids::ChatRunId::new("turn-tool-result"),
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
        context: RuntimeRunContext::new(
            sdk::ids::ChatId::new("chat-1"),
            sdk::ids::ChatRunId::new("turn-1"),
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
    let context = RuntimeRunContext::new(
        sdk::ids::ChatId::new("chat-retry"),
        sdk::ids::ChatRunId::new("turn-retry"),
    );
    let expected_chat_id = context.chat_id.clone();
    let expected_run_id = context.run_id.clone();
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
            assert_eq!(context.run_id, expected_run_id);
            assert_eq!(attempt, 2);
            assert_eq!(delay, std::time::Duration::from_millis(10_250));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

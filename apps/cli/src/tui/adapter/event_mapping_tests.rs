use super::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivitySource, TuiActivityState, TuiCompactStage, TuiHookPoint, TuiInteractionKind,
    TuiModelStreamState, TuiRunPhaseKind, TuiRunPurpose, TuiRuntimeEvent,
};

#[test]
fn session_message_state_maps_count_and_revision_without_messages() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::SessionMessageStateChanged {
        message_count: 7,
        revision: 3,
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::SessionMessageStateChanged {
            message_count: 7,
            revision: 3,
        })
    ));
}

#[test]
fn hook_notice_maps_point_kind_and_complete_payload() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::HookNotice {
        notice: sdk::HookNoticeView {
            point: "PreToolUse".to_string(),
            kind: sdk::HookNoticeKindView::Blocked,
            summary: "Stop hook 阻止了停止。".to_string(),
            command: "check-agent-stop.sh".to_string(),
            exit_code: Some(2),
            reason: "exit code 2".to_string(),
            stdout_preview: "stdout preview".to_string(),
            stderr_preview: "stderr preview".to_string(),
            stdout_truncated: true,
            stderr_truncated: false,
            output_file: Some("/tmp/stop-hook.txt".to_string()),
        },
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::HookNotice(feedback))
            if feedback.point == "PreToolUse"
                && feedback.kind == crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked
                && feedback.summary == "Stop hook 阻止了停止。"
                && feedback.command == "check-agent-stop.sh"
                && feedback.exit_code == Some(2)
                && feedback.reason == "exit code 2"
                && feedback.stdout_preview == "stdout preview"
                && feedback.stderr_preview == "stderr preview"
                && feedback.stdout_truncated
                && !feedback.stderr_truncated
                && feedback.output_file.as_deref() == Some("/tmp/stop-hook.txt")
    ));
}

#[test]
fn activity_increment_maps_complete_typed_fact_without_sdk_types() {
    let run_id = sdk::RunId::new("run-activity");
    let step_id = sdk::RunStepId::new("step-activity");
    let activity_id = sdk::ActivityId::new("activity-model");
    let parent_activity_id = sdk::ActivityId::new("activity-root");
    let model_invocation_id = sdk::ModelInvocationId::new("model-invocation");
    let expected_model_invocation_id = model_invocation_id.as_str().to_string();
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ActivityChanged {
        kind: sdk::ActivityChangeKind::Updated,
        activity: sdk::ActivityView {
            id: activity_id.clone(),
            run_id: run_id.clone(),
            run_step_id: Some(step_id.clone()),
            parent_activity_id: Some(parent_activity_id.clone()),
            source: sdk::ActivitySourceView::ModelInvocation(model_invocation_id),
            kind: sdk::ActivityKindView::ModelInvocation,
            state: sdk::ActivityStateView::Waiting,
            detail: sdk::ActivityDetailView::Model {
                model: "claude-sonnet".to_string(),
                attempt: 2,
                stream: sdk::ModelStreamStateView::Retrying,
            },
            audience: sdk::ActivityAudienceView::Operational,
            revision: 7,
            timing: sdk::ActivityTimingView {
                total_elapsed_ms: 1_500,
                active_elapsed_ms: 1_000,
                state_elapsed_ms: 500,
                started_at_unix_ms: Some(100),
                finished_at_unix_ms: None,
            },
        },
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ActivityChanged {
            kind: TuiActivityChangeKind::Updated,
            activity,
        }) if activity.id.as_str() == activity_id.as_str()
            && activity.run_id.as_str() == run_id.as_str()
            && activity.run_step_id.as_ref().is_some_and(|id| id.as_str() == step_id.as_str())
            && activity.parent_activity_id.as_ref().is_some_and(|id| id.as_str() == parent_activity_id.as_str())
            && matches!(activity.source, TuiActivitySource::ModelInvocation(ref id) if id == &expected_model_invocation_id)
            && activity.kind == TuiActivityKind::ModelInvocation
            && activity.state == TuiActivityState::Waiting
            && matches!(activity.detail, TuiActivityDetail::Model { ref model, attempt: 2, stream: TuiModelStreamState::Retrying } if model == "claude-sonnet")
            && activity.audience == TuiActivityAudience::Operational
            && activity.revision == 7
            && activity.timing.total_elapsed_ms == 1_500
            && activity.timing.active_elapsed_ms == 1_000
            && activity.timing.state_elapsed_ms == 500
            && activity.timing.started_at_unix_ms == Some(100)
            && activity.timing.finished_at_unix_ms.is_none()
    ));
}

#[test]
fn activity_snapshot_maps_all_closed_enum_variants() {
    let run_id = sdk::RunId::new("run-snapshot");
    let fixture = |index: usize,
                   source: sdk::ActivitySourceView,
                   kind: sdk::ActivityKindView,
                   state: sdk::ActivityStateView,
                   detail: sdk::ActivityDetailView,
                   audience: sdk::ActivityAudienceView| {
        sdk::ActivityView {
            id: sdk::ActivityId::new(format!("activity-{index}")),
            run_id: run_id.clone(),
            run_step_id: None,
            parent_activity_id: None,
            source,
            kind,
            state,
            detail,
            audience,
            revision: index as u64 + 1,
            timing: sdk::ActivityTimingView::default(),
        }
    };
    let tool_call_id = sdk::ToolCallId::new("tool-call");
    let expected_tool_call_id = tool_call_id.as_str().to_string();
    let interaction_id = sdk::InteractionRequestId::new("interaction");
    let expected_interaction_id = interaction_id.as_str().to_string();
    let phase_step_id = sdk::RunStepId::new("phase-step");
    let expected_phase_step_id = phase_step_id.as_str().to_string();
    let hook_dispatch_id = sdk::ActivityId::new("hook-dispatch");
    let expected_hook_dispatch_id = hook_dispatch_id.as_str().to_string();
    let compaction_id = sdk::ActivityId::new("compaction");
    let expected_compaction_id = compaction_id.as_str().to_string();
    let child_run_id = sdk::RunId::new("child-run");
    let expected_child_run_id = child_run_id.as_str().to_string();
    let activities = vec![
        fixture(
            0,
            sdk::ActivitySourceView::Run,
            sdk::ActivityKindView::Run,
            sdk::ActivityStateView::Running,
            sdk::ActivityDetailView::Run {
                purpose: sdk::RunPurposeView::Main,
            },
            sdk::ActivityAudienceView::User,
        ),
        fixture(
            1,
            sdk::ActivitySourceView::RunStep(phase_step_id),
            sdk::ActivityKindView::RunPhase(sdk::RunPhaseKindView::Terminating),
            sdk::ActivityStateView::Succeeded,
            sdk::ActivityDetailView::Phase {
                phase: sdk::RunPhaseKindView::CancellingStep,
            },
            sdk::ActivityAudienceView::Diagnostic,
        ),
        fixture(
            2,
            sdk::ActivitySourceView::ToolCall(tool_call_id),
            sdk::ActivityKindView::ToolCall,
            sdk::ActivityStateView::Failed,
            sdk::ActivityDetailView::Tool {
                name: "Bash".to_string(),
                summary: Some("cargo test".to_string()),
                parallel_count: 3,
            },
            sdk::ActivityAudienceView::User,
        ),
        fixture(
            3,
            sdk::ActivitySourceView::HookDispatch(hook_dispatch_id),
            sdk::ActivityKindView::HookDispatch,
            sdk::ActivityStateView::Cancelled,
            sdk::ActivityDetailView::Hook {
                point: sdk::HookPointView::StopFailure,
                script: "check-stop-failure.sh".to_string(),
                attempt: 4,
            },
            sdk::ActivityAudienceView::Diagnostic,
        ),
        fixture(
            4,
            sdk::ActivitySourceView::Interaction(interaction_id),
            sdk::ActivityKindView::Interaction,
            sdk::ActivityStateView::Terminated,
            sdk::ActivityDetailView::Interaction {
                kind: sdk::InteractionKindView::PlanApproval,
            },
            sdk::ActivityAudienceView::Operational,
        ),
        fixture(
            5,
            sdk::ActivitySourceView::ChildRun(child_run_id),
            sdk::ActivityKindView::ChildRun,
            sdk::ActivityStateView::Waiting,
            sdk::ActivityDetailView::ChildRun {
                role: "reviewer".to_string(),
                model: "claude-opus".to_string(),
            },
            sdk::ActivityAudienceView::User,
        ),
        fixture(
            6,
            sdk::ActivitySourceView::Compaction(compaction_id),
            sdk::ActivityKindView::Compaction,
            sdk::ActivityStateView::Running,
            sdk::ActivityDetailView::Compact {
                stage: sdk::CompactStageView::Finalizing,
                current: Some(2),
                total: Some(3),
            },
            sdk::ActivityAudienceView::Operational,
        ),
    ];

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ActivitySnapshot(
        sdk::ActivitySnapshotView {
            run_id: run_id.clone(),
            revision: 11,
            activities,
        },
    ));

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ActivitySnapshot(snapshot))
            if snapshot.run_id.as_str() == run_id.as_str()
                && snapshot.revision == 11
                && snapshot.activities.len() == 7
                && snapshot.activities[0].source == TuiActivitySource::Run
                && matches!(snapshot.activities[0].detail, TuiActivityDetail::Run { purpose: TuiRunPurpose::Main })
                && matches!(snapshot.activities[1].source, TuiActivitySource::RunStep(ref id) if id.as_str() == expected_phase_step_id)
                && snapshot.activities[1].kind == TuiActivityKind::RunPhase(TuiRunPhaseKind::Terminating)
                && matches!(snapshot.activities[1].detail, TuiActivityDetail::Phase { phase: TuiRunPhaseKind::CancellingStep })
                && matches!(snapshot.activities[2].source, TuiActivitySource::ToolCall(ref id) if id == &expected_tool_call_id)
                && matches!(snapshot.activities[2].detail, TuiActivityDetail::Tool { ref name, parallel_count: 3, .. } if name == "Bash")
                && matches!(snapshot.activities[3].source, TuiActivitySource::HookDispatch(ref id) if id.as_str() == expected_hook_dispatch_id)
                && matches!(snapshot.activities[3].detail, TuiActivityDetail::Hook { point: TuiHookPoint::StopFailure, ref script, attempt: 4 } if script == "check-stop-failure.sh")
                && matches!(snapshot.activities[4].source, TuiActivitySource::Interaction(ref id) if id == &expected_interaction_id)
                && matches!(snapshot.activities[4].detail, TuiActivityDetail::Interaction { kind: TuiInteractionKind::PlanApproval })
                && matches!(snapshot.activities[5].source, TuiActivitySource::ChildRun(ref id) if id.as_str() == expected_child_run_id)
                && matches!(snapshot.activities[5].detail, TuiActivityDetail::ChildRun { ref role, ref model } if role == "reviewer" && model == "claude-opus")
                && matches!(snapshot.activities[6].source, TuiActivitySource::Compaction(ref id) if id.as_str() == expected_compaction_id)
                && matches!(snapshot.activities[6].detail, TuiActivityDetail::Compact { stage: TuiCompactStage::Finalizing, current: Some(2), total: Some(3) })
    ));
}

#[test]
fn activity_closed_enum_helpers_map_every_variant() {
    let phases = [
        (
            sdk::RunPhaseKindView::DrainingInput,
            TuiRunPhaseKind::DrainingInput,
        ),
        (
            sdk::RunPhaseKindView::PreparingContext,
            TuiRunPhaseKind::PreparingContext,
        ),
        (
            sdk::RunPhaseKindView::ApplyingResponse,
            TuiRunPhaseKind::ApplyingResponse,
        ),
        (
            sdk::RunPhaseKindView::AwaitingToolApproval,
            TuiRunPhaseKind::AwaitingToolApproval,
        ),
        (
            sdk::RunPhaseKindView::ExecutingTools,
            TuiRunPhaseKind::ExecutingTools,
        ),
        (
            sdk::RunPhaseKindView::FinalizingStep,
            TuiRunPhaseKind::FinalizingStep,
        ),
        (
            sdk::RunPhaseKindView::CancellingStep,
            TuiRunPhaseKind::CancellingStep,
        ),
        (
            sdk::RunPhaseKindView::Terminating,
            TuiRunPhaseKind::Terminating,
        ),
    ];
    for (sdk_phase, expected) in phases {
        assert_eq!(super::run_phase(sdk_phase), expected);
    }

    let hook_points = [
        sdk::HookPointView::PreToolUse,
        sdk::HookPointView::UserPromptSubmit,
        sdk::HookPointView::PreCompact,
        sdk::HookPointView::PermissionRequest,
        sdk::HookPointView::Elicitation,
        sdk::HookPointView::UserPromptExpansion,
        sdk::HookPointView::Stop,
        sdk::HookPointView::PostToolUse,
        sdk::HookPointView::PostToolUseFailure,
        sdk::HookPointView::PostCompact,
        sdk::HookPointView::PostToolBatch,
        sdk::HookPointView::ElicitationResult,
        sdk::HookPointView::SessionStart,
        sdk::HookPointView::SessionEnd,
        sdk::HookPointView::SubRunStart,
        sdk::HookPointView::SubRunStop,
        sdk::HookPointView::TaskCreated,
        sdk::HookPointView::TaskCompleted,
        sdk::HookPointView::Notification,
        sdk::HookPointView::InstructionsLoaded,
        sdk::HookPointView::StopFailure,
        sdk::HookPointView::PermissionDenied,
        sdk::HookPointView::ConfigChange,
        sdk::HookPointView::CwdChanged,
        sdk::HookPointView::FileChanged,
        sdk::HookPointView::TeammateIdle,
    ];
    for sdk_point in hook_points {
        let mapped = super::hook_point(sdk_point);
        assert!(matches!(
            mapped,
            TuiHookPoint::PreToolUse
                | TuiHookPoint::UserPromptSubmit
                | TuiHookPoint::PreCompact
                | TuiHookPoint::PermissionRequest
                | TuiHookPoint::Elicitation
                | TuiHookPoint::UserPromptExpansion
                | TuiHookPoint::Stop
                | TuiHookPoint::PostToolUse
                | TuiHookPoint::PostToolUseFailure
                | TuiHookPoint::PostCompact
                | TuiHookPoint::PostToolBatch
                | TuiHookPoint::ElicitationResult
                | TuiHookPoint::SessionStart
                | TuiHookPoint::SessionEnd
                | TuiHookPoint::SubRunStart
                | TuiHookPoint::SubRunStop
                | TuiHookPoint::TaskCreated
                | TuiHookPoint::TaskCompleted
                | TuiHookPoint::Notification
                | TuiHookPoint::InstructionsLoaded
                | TuiHookPoint::StopFailure
                | TuiHookPoint::PermissionDenied
                | TuiHookPoint::ConfigChange
                | TuiHookPoint::CwdChanged
                | TuiHookPoint::FileChanged
                | TuiHookPoint::TeammateIdle
        ));
    }
}

#[test]
fn tool_result_projection_keeps_bounded_payload_and_blob_reason() {
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
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ToolResult {
        context: sdk::ChatEventContext::new(
            sdk::ChatId::new("chat-tool-result"),
            sdk::ChatRunId::new("turn-tool-result"),
        ),
        id: sdk::ToolCallId::new("runtime-call"),
        provider_id: "provider-call".to_string(),
        tool_name: "Bash".to_string(),
        output: "bounded preview".to_string(),
        content: content.clone(),
        is_error: false,
        images: Vec::new(),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ToolResult {
            output,
            content: projected,
            ..
        }) if output == "bounded preview"
            && projected == content
            && projected.pointer("/blob/reason").and_then(serde_json::Value::as_str)
                == Some("write_failed")
    ));
}

#[test]
fn session_resume_keeps_context_run_step_boundaries_and_all_terminal_causes() {
    for (sdk_cause, tui_cause) in [
        (
            sdk::ResumedStepFinalizeCause::Completed,
            super::super::runtime_view::TuiResumedStepFinalizeCause::Completed,
        ),
        (
            sdk::ResumedStepFinalizeCause::UserCancelledStep,
            super::super::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep,
        ),
        (
            sdk::ResumedStepFinalizeCause::RunTerminated,
            super::super::runtime_view::TuiResumedStepFinalizeCause::RunTerminated,
        ),
    ] {
        let mapped = sdk_event_to_tui_event(sdk::ChatEvent::SessionResumed {
            steps: vec![sdk::ResumedSessionStep {
                run_id: "run-1".into(),
                step_id: "step-1".into(),
                messages: vec![sdk::ChatMessage::user_text("hello")],
                finalize_cause: Some(sdk_cause),
                duration_ms: Some(125_000),
            }],
            display_history: None,
            session_id: "session-1".into(),
            created_at: 0,
            compacted: false,
        });
        assert!(matches!(
            mapped,
            SdkEventMapping::Runtime(TuiRuntimeEvent::SessionResumed { steps, .. })
                if steps.len() == 1
                    && steps[0].run_id == "run-1"
                    && steps[0].step_id == "step-1"
                    && steps[0].finalize_cause == Some(tui_cause)
                    && steps[0].duration_ms == Some(125_000)
                    && steps[0].messages[0].text_content() == "hello"
        ));
    }
}

#[test]
fn authoritative_cancelled_terminal_maps_without_run_or_step_correlation() {
    let chat_id = sdk::ChatId::new("chat-cancelled");
    let run_id = sdk::ChatRunId::new("turn-cancelled");

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::Cancelled {
        context: sdk::ChatEventContext::new(chat_id.clone(), run_id.clone()),
        duration_ms: 6_000,
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::Cancelled {
            context,
            duration_ms: 6_000,
        }) if context.chat_id == chat_id.as_str() && context.run_id == run_id.as_str()
    ));
}

#[test]
fn interaction_request_keeps_request_run_and_body_identity() {
    let request_id = sdk::InteractionRequestId::new("request-1");
    let run_id = sdk::RunId::new("run-1");
    let expected_request_id = request_id.as_str().to_string();
    let expected_run_id = run_id.as_str().to_string();
    let request = sdk::InteractionRequest {
        id: request_id,
        run_id,
        tool_call_id: None,
        body: sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
            tool_name: "Bash".to_string(),
            args_summary: "rm -rf target".to_string(),
            risk_level: sdk::RiskLevel::High,
        }),
    };

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::InteractionRequested { request });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::InteractionRequested(request))
            if request.request_id.as_str() == expected_request_id
                && request.run_id.as_str() == expected_run_id
    ));
}

#[test]
fn ask_user_batch_is_retired_and_mapped_to_nop() {
    let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::AskUserBatch {
        items: Vec::new(),
        reply_tx,
    });

    assert!(matches!(mapped, SdkEventMapping::Nop));
}

#[test]
fn task_state_preserves_structured_payload() {
    let expected = sdk::TaskStateView::empty("session-a", 42);
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::TaskStateChanged {
        state: Box::new(expected.clone()),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::TaskStateChanged { state })
            if state.session_id == "session-a" && state.revision == 42 && state.items.is_empty()
    ));
}

#[test]
fn config_reload_maps_spacing_policy_into_tui_owned_event() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ConfigReloaded {
        event: sdk::ConfigReloadedEvent {
            changed_keys: vec!["ui.markdown_spacing".to_string()],
            scopes: vec![sdk::ConfigApplicationScopeView::Immediate],
            view: sdk::ConfigView {
                markdown_spacing: sdk::MarkdownSpacingModeView::Compact,
                ..Default::default()
            },
        },
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ConfigReloaded { view, .. })
            if view.markdown_spacing.mode()
                == crate::tui::render::output::spacing::MarkdownSpacingMode::Compact
    ));
}

#[test]
fn config_reload_preserves_permission_mode_in_tui_owned_event() {
    for permission_mode in ["ask", "auto_read", "allow_all"] {
        let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ConfigReloaded {
            event: sdk::ConfigReloadedEvent {
                changed_keys: vec!["permissions.mode".to_string()],
                scopes: vec![sdk::ConfigApplicationScopeView::Run],
                view: sdk::ConfigView {
                    permission_mode: permission_mode.to_string(),
                    ..Default::default()
                },
            },
        });

        assert!(matches!(
            mapped,
            SdkEventMapping::Runtime(TuiRuntimeEvent::ConfigReloaded { view, .. })
                if view.permission_mode == permission_mode
        ));
    }
}

#[test]
fn model_invocation_retry_mapping_preserves_context_attempt_and_delay() {
    let expected_chat_id = sdk::ids::ChatId::new("chat-retry");
    let expected_run_id = sdk::ids::ChatRunId::new("turn-retry");
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ModelInvocationRetrying {
        context: sdk::ChatEventContext::new(expected_chat_id.clone(), expected_run_id.clone()),
        attempt: 2,
        delay: std::time::Duration::from_millis(10_250),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::ModelInvocationRetrying {
            context,
            attempt: 2,
            delay_ms: 10_250,
        }) if context.chat_id == expected_chat_id.as_str()
            && context.run_id == expected_run_id.as_str()
    ));
}

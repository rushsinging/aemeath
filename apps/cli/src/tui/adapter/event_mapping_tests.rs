use super::{sdk_event_to_tui_event, SdkEventMapping};
use crate::tui::adapter::tui_runtime_event::{TuiHookStatus, TuiRunEvent, TuiRuntimeEvent};

#[test]
fn session_resume_keeps_context_run_step_boundaries() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::SessionResumed {
        steps: vec![sdk::ResumedSessionStep {
            run_id: "run-1".into(),
            step_id: "step-1".into(),
            messages: vec![sdk::ChatMessage::user_text("hello")],
            finalize_cause: Some(sdk::ResumedStepFinalizeCause::UserCancelledStep),
            duration_ms: Some(125_000),
        }],
        session_id: "session-1".into(),
        created_at: 0,
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::SessionResumed { steps, .. })
            if steps.len() == 1
                && steps[0].run_id == "run-1"
                && steps[0].step_id == "step-1"
                && steps[0].finalize_cause == Some(super::super::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep)
                && steps[0].duration_ms == Some(125_000)
                && steps[0].messages[0].text_content() == "hello"
    ));
}

#[test]
fn authoritative_cancelled_terminal_maps_without_run_or_step_correlation() {
    let chat_id = sdk::ChatId::new("chat-cancelled");
    let turn_id = sdk::ChatTurnId::new("turn-cancelled");

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::Cancelled {
        context: sdk::ChatEventContext::new(chat_id.clone(), turn_id.clone()),
        duration_ms: 6_000,
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::Cancelled {
            context,
            duration_ms: 6_000,
        }) if context.chat_id == chat_id.as_str() && context.turn_id == turn_id.as_str()
    ));
}

#[test]
fn run_cancelling_keeps_identity_instead_of_becoming_empty_message() {
    let run_id = sdk::RunId::new("run-1");

    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::RunCancelling {
        run_id: run_id.clone(),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::Run {
            run_id: actual,
            parent_run_id: None,
            event: TuiRunEvent::Cancelling,
        }) if actual.as_str() == run_id.as_str()
    ));
}

#[test]
fn hook_event_preserves_authoritative_status_and_final_diagnostics() {
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::HookEvent(sdk::HookEventView {
        hook_name: "Stop".to_string(),
        status: sdk::HookEventStatus::Succeeded,
        matcher: Some("*".to_string()),
        command: Some("check-agent-stop.sh".to_string()),
        result: Some(sdk::HookExecutionResultView {
            exit_code: Some(0),
            stdout: "ok".to_string(),
            stderr: String::new(),
            decision: Some("continue".to_string()),
            reason: None,
            additional_context: None,
        }),
    }));

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::HookEvent(event))
            if event.status == TuiHookStatus::Succeeded
                && event.matcher.as_deref() == Some("*")
                && event.command.as_deref() == Some("check-agent-stop.sh")
                && event.result.as_ref().is_some_and(|result|
                    result.exit_code == Some(0)
                        && result.stdout == "ok"
                        && result.stderr.is_empty()
                        && result.decision.as_deref() == Some("continue")
                        && result.reason.is_none())
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
fn tasks_snapshot_preserves_sequence_prefixed_lines() {
    let expected = vec![
        "━━ Tasks: 0/1 ━━".to_string(),
        "□ #1 实现适配器".to_string(),
    ];
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::TasksSnapshot {
        tasks: Box::new(sdk::TaskStatusView {
            lines: expected.clone(),
        }),
    });

    assert!(matches!(
        mapped,
        SdkEventMapping::Runtime(TuiRuntimeEvent::TasksSnapshot { lines }) if lines == expected
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
    let expected_turn_id = sdk::ids::ChatTurnId::new("turn-retry");
    let mapped = sdk_event_to_tui_event(sdk::ChatEvent::ModelInvocationRetrying {
        context: sdk::ChatEventContext::new(expected_chat_id.clone(), expected_turn_id.clone()),
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
            && context.turn_id == expected_turn_id.as_str()
    ));
}

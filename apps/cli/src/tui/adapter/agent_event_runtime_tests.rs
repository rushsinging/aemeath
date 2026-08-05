use super::map_runtime_event;
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySnapshot, TuiActivitySource, TuiActivityState,
    TuiActivityTiming, TuiInteractionBody, TuiInteractionRequest, TuiRunEvent, TuiRunPurpose,
    TuiRunStepEvent, TuiRuntimeEvent, TuiToolApprovalPrompt, TuiWorkspaceSnapshot, UiActivityId,
};
use crate::tui::model::conversation::intent::{
    ConversationIntent, ObserveActivityChange, PresentCancelledStep, ReplaceActivitySnapshot,
    ShowInteraction,
};
use crate::tui::model::conversation::interaction::{
    UiInteractionRequestId, UiRiskLevel, UiRunId, UiRunStepId,
};
use crate::tui::model::workspace_provider::WorkspaceIntent;

fn activity(run_id: &str, activity_id: &str, revision: u64) -> TuiActivityObservation {
    TuiActivityObservation {
        id: UiActivityId::from(activity_id),
        run_id: UiRunId::from(run_id),
        run_step_id: None,
        parent_activity_id: None,
        source: TuiActivitySource::Run,
        kind: TuiActivityKind::Run,
        state: TuiActivityState::Running,
        detail: TuiActivityDetail::Run {
            purpose: TuiRunPurpose::Main,
        },
        audience: TuiActivityAudience::User,
        revision,
        timing: TuiActivityTiming::default(),
    }
}

#[test]
fn activity_events_map_to_dedicated_conversation_intents() {
    let changed = map_runtime_event(&TuiRuntimeEvent::ActivityChanged {
        kind: TuiActivityChangeKind::Updated,
        activity: activity("run-1", "activity-1", 2),
    });
    assert!(matches!(
        changed.conversation.as_slice(),
        [ConversationIntent::ObserveActivityChange(ObserveActivityChange {
            kind: TuiActivityChangeKind::Updated,
            activity,
        })] if activity.id.as_str() == "activity-1" && activity.revision == 2
    ));

    let snapshot = map_runtime_event(&TuiRuntimeEvent::ActivitySnapshot(TuiActivitySnapshot {
        run_id: UiRunId::from("run-1"),
        revision: 3,
        activities: vec![activity("run-1", "activity-1", 3)],
    }));
    assert!(matches!(
        snapshot.conversation.as_slice(),
        [ConversationIntent::ReplaceActivitySnapshot(ReplaceActivitySnapshot { snapshot })]
            if snapshot.run_id.as_str() == "run-1"
                && snapshot.revision == 3
                && snapshot.activities.len() == 1
    ));
}

#[test]
fn runtime_run_and_step_lifecycle_are_observational_only() {
    let run_id = UiRunId::from("run-1");
    let run_mapping = map_runtime_event(&TuiRuntimeEvent::Run {
        run_id: run_id.clone(),
        parent_run_id: None,
        event: TuiRunEvent::TerminationRequested {
            reason: crate::tui::adapter::tui_runtime_event::TuiRunTerminationReason::UserExit,
            deadline_unix_millis: 42,
        },
    });
    assert!(run_mapping.conversation.is_empty());

    let step_mapping = map_runtime_event(&TuiRuntimeEvent::RunStep {
        run_id,
        parent_run_id: None,
        step_id: UiRunStepId::from("step-1"),
        event: TuiRunStepEvent::CancellationRequested,
    });
    assert!(step_mapping.conversation.is_empty());
}

#[test]
fn runtime_cancelled_step_maps_to_presentation_only_intent() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::RunStep {
        run_id: UiRunId::from("run-1"),
        parent_run_id: None,
        step_id: UiRunStepId::from("step-1"),
        event: TuiRunStepEvent::Cancelled {
            terminal:
                crate::tui::adapter::tui_runtime_event::TuiRunStepCancellationTerminal::Cancelled,
        },
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::PresentCancelledStep(
            PresentCancelledStep { confirmed: true }
        )]
    ));
}

#[test]
fn child_cancelled_step_remains_observational_only() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::RunStep {
        run_id: UiRunId::from("child-run"),
        parent_run_id: Some(UiRunId::from("root-run")),
        step_id: UiRunStepId::from("child-step"),
        event: TuiRunStepEvent::Cancelled {
            terminal:
                crate::tui::adapter::tui_runtime_event::TuiRunStepCancellationTerminal::Cancelled,
        },
    });

    assert!(mapping.conversation.is_empty());
}

#[test]
fn runtime_interaction_maps_to_sender_free_show_interaction() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: UiInteractionRequestId::from("request-1"),
            run_id: UiRunId::from("run-1"),
            tool_call_id: None,
            body: TuiInteractionBody::ToolApproval(TuiToolApprovalPrompt {
                tool_name: "Bash".to_string(),
                args_summary: "rm -rf target".to_string(),
                risk_level: crate::tui::adapter::tui_runtime_event::TuiRiskLevel::High,
            }),
        },
    ));

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ShowInteraction(ShowInteraction { request })]
            if request.request_id.as_str() == "request-1"
                && request.run_id.as_str() == "run-1"
                && matches!(request.body, crate::tui::model::conversation::interaction::InteractionBody::ToolApproval(ref prompt) if prompt.risk == UiRiskLevel::High)
    ));
}

#[test]
fn compact_finished_syncs_messages_and_appends_runtime_notice_once() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::CompactFinished {
        messages: Vec::new(),
        notice: "✓ 上下文压缩完成".to_string(),
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::AppendSystemMessage(message)]
            if message.text == "✓ 上下文压缩完成"
    ));
    assert_eq!(mapping.session.len(), 1);
}

#[test]
fn runtime_workspace_snapshot_maps_without_git_metadata() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::WorkspaceSnapshot(TuiWorkspaceSnapshot {
        path_base: "/repo/.worktrees/feature".to_string(),
        workspace_root: "/repo".to_string(),
        context_stack: vec![("/repo".to_string(), "/repo".to_string())],
    }));

    assert_eq!(
        mapping.workspace,
        vec![WorkspaceIntent::ApplySnapshot {
            path_base: Some("/repo/.worktrees/feature".to_string()),
            workspace_root: Some("/repo".to_string()),
        }]
    );
}

#[test]
fn runtime_tool_progress_maps_to_record_tool_streaming_output_intent() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::ToolProgress {
        context: crate::tui::adapter::tui_runtime_event::TuiRunContext {
            chat_id: "chat-1".to_string(),
            run_id: "run-1".to_string(),
        },
        tool_id: "bash-1".to_string(),
        event: crate::tui::adapter::tui_runtime_event::TuiToolProgressEvent {
            text: "line of stdout\n".to_string(),
        },
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::RecordToolStreamingOutput(intent)]
            if intent.chat_id.as_ref() == "chat-1"
                && intent.run_id.as_ref() == "run-1"
                && intent.tool_id.as_ref() == "bash-1"
                && intent.text == "line of stdout\n"
    ));
}

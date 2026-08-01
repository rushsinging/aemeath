use super::map_runtime_event;
use crate::tui::adapter::tui_runtime_event::{
    TuiActivityAudience, TuiActivityChangeKind, TuiActivityDetail, TuiActivityKind,
    TuiActivityObservation, TuiActivitySnapshot, TuiActivitySource, TuiActivityState,
    TuiActivityTiming, TuiInteractionBody, TuiInteractionRequest, TuiRunEvent, TuiRunPurpose,
    TuiRunStatus, TuiRunStepEvent, TuiRuntimeEvent, TuiToolApprovalPrompt, TuiWorkspaceSnapshot,
    UiActivityId,
};
use crate::tui::model::conversation::intent::{
    ConversationIntent, ObserveActivityChange, ObserveRunStatus, ReplaceActivitySnapshot,
    RunCancelling, RunStepStarted, ShowInteraction,
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
fn runtime_run_and_step_lifecycle_maps_to_existing_conversation_intents() {
    let run_id = UiRunId::from("run-1");
    let mapping = map_runtime_event(&TuiRuntimeEvent::Run {
        run_id: run_id.clone(),
        parent_run_id: None,
        event: TuiRunEvent::Cancelling,
    });
    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::RunCancelling(RunCancelling { run_id: actual })] if actual == &run_id
    ));

    let mapping = map_runtime_event(&TuiRuntimeEvent::RunStep {
        run_id: run_id.clone(),
        parent_run_id: None,
        step_id: UiRunStepId::from("step-1"),
        event: TuiRunStepEvent::Started,
    });
    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::RunStepStarted(RunStepStarted { run_id: actual, step_id, .. })]
            if actual == &run_id && step_id.as_str() == "step-1"
    ));
}

#[test]
fn transitioned_run_maps_to_status_observation() {
    let run_id = UiRunId::from("run-1");
    let parent_run_id = UiRunId::from("parent-1");
    let mapping = map_runtime_event(&TuiRuntimeEvent::Run {
        run_id: run_id.clone(),
        parent_run_id: Some(parent_run_id.clone()),
        event: TuiRunEvent::Transitioned {
            status: TuiRunStatus::InvokingModel,
            timing: crate::tui::adapter::tui_runtime_event::TuiRunTiming {
                observation_revision: 1,
                total_elapsed_ms: 12_345,
                phase_elapsed_ms: 678,
            },
        },
    });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::ObserveRunStatus(ObserveRunStatus {
            run_id: actual_run_id,
            parent_run_id: Some(actual_parent_run_id),
            status: TuiRunStatus::InvokingModel,
            timing: crate::tui::adapter::tui_runtime_event::TuiRunTiming {
                observation_revision: 1,
                total_elapsed_ms: 12_345,
                phase_elapsed_ms: 678,
            },
        })] if actual_run_id == &run_id && actual_parent_run_id == &parent_run_id    ));
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

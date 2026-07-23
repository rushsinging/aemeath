use super::{map_runtime_event, AgentEventMapping};
use crate::tui::adapter::tui_runtime_event::{
    TuiInteractionBody, TuiInteractionRequest, TuiRunEvent, TuiRunStepEvent, TuiRuntimeEvent,
    TuiToolApprovalPrompt, TuiWorkspaceSnapshot,
};
use crate::tui::model::conversation::intent::{
    ConversationIntent, RunCancelling, RunStepStarted, ShowInteraction,
};
use crate::tui::model::conversation::interaction::{
    UiInteractionRequestId, UiRiskLevel, UiRunId, UiRunStepId,
};
use crate::tui::model::workspace_provider::WorkspaceIntent;

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
fn runtime_interaction_maps_to_sender_free_show_interaction() {
    let mapping = map_runtime_event(&TuiRuntimeEvent::InteractionRequested(
        TuiInteractionRequest {
            request_id: UiInteractionRequestId::from("request-1"),
            run_id: UiRunId::from("run-1"),
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

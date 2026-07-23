use super::intent::{
    CancelInteraction, ConfirmInteraction, ConversationIntent, RunAwaitingUser, RunCancelled,
    RunCancelling, RunCompleted, RunFailed, RunResumed, RunStarted, RunStepCompleted,
    RunStepStarted,
};
use super::interaction::{
    AgentRunPhase, AgentRunStepPhase, InteractionBody, InteractionRequest, UiInteractionRequestId,
    UiRunId, UiRunStepId, UiStuckDiagnostic,
};
use super::model::ConversationModel;

#[test]
fn lifecycle_transitions_only_follow_runtime_authority() {
    let mut model = ConversationModel::default();
    let run_id = UiRunId::from("run-1");

    model.apply(ConversationIntent::RunStarted(RunStarted {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::RunResumed(RunResumed {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::RunCancelling(RunCancelling {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::RunCancelled(RunCancelled {
        run_id: run_id.clone(),
    }));

    assert_eq!(
        model.agent_run(&run_id).unwrap().phase(),
        AgentRunPhase::Cancelled
    );
}

#[test]
fn interaction_command_result_does_not_resume_or_cancel_agent_run() {
    let mut model = ConversationModel::default();
    let run_id = UiRunId::from("run-1");
    let request_id = UiInteractionRequestId::from("request-1");
    model.apply(ConversationIntent::RunStarted(RunStarted {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
        run_id: run_id.clone(),
    }));
    model.apply(ConversationIntent::ShowInteraction(
        super::intent::ShowInteraction {
            request: InteractionRequest {
                request_id: request_id.clone(),
                run_id: run_id.clone(),
                body: InteractionBody::HardPause(UiStuckDiagnostic {
                    reason: "等待继续".to_string(),
                    recent_actions: Vec::new(),
                }),
            },
        },
    ));

    model.apply(ConversationIntent::ConfirmInteraction(ConfirmInteraction {
        request_id: request_id.clone(),
    }));
    model.apply(ConversationIntent::InteractionReplyAccepted(
        super::intent::InteractionReplyAccepted {
            request_id: request_id.clone(),
        },
    ));
    model.apply(ConversationIntent::CancelInteraction(CancelInteraction {
        request_id,
    }));

    assert_eq!(
        model.agent_run(&run_id).unwrap().phase(),
        AgentRunPhase::AwaitingUser
    );
}

#[test]
fn run_steps_preserve_order_and_complete_in_place() {
    let mut model = ConversationModel::default();
    let run_id = UiRunId::from("run-1");
    model.apply(ConversationIntent::RunStarted(RunStarted {
        run_id: run_id.clone(),
    }));
    for step_id in ["step-1", "step-2"] {
        model.apply(ConversationIntent::RunStepStarted(RunStepStarted {
            run_id: run_id.clone(),
            step_id: UiRunStepId::from(step_id),
            tool_reference: None,
        }));
    }

    model.apply(ConversationIntent::RunStepCompleted(RunStepCompleted {
        run_id: run_id.clone(),
        step_id: UiRunStepId::from("step-1"),
    }));

    let steps = model.agent_run(&run_id).unwrap().steps();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].step_id().as_str(), "step-1");
    assert_eq!(steps[0].phase(), AgentRunStepPhase::Completed);
    assert_eq!(steps[1].step_id().as_str(), "step-2");
    assert_eq!(steps[1].phase(), AgentRunStepPhase::Running);
}

#[test]
fn unknown_run_step_is_ignored_without_creating_agent_run() {
    let mut model = ConversationModel::default();
    let run_id = UiRunId::from("missing-run");

    let changes = model.apply(ConversationIntent::RunStepCompleted(RunStepCompleted {
        run_id: run_id.clone(),
        step_id: UiRunStepId::from("step-1"),
    }));

    assert!(changes.is_empty());
    assert!(model.agent_run(&run_id).is_none());
}

#[test]
fn running_run_can_complete_or_fail_but_awaiting_run_cannot() {
    let mut model = ConversationModel::default();
    let complete_id = UiRunId::from("complete-run");
    let failed_id = UiRunId::from("failed-run");
    let awaiting_id = UiRunId::from("awaiting-run");

    for run_id in [&complete_id, &failed_id, &awaiting_id] {
        model.apply(ConversationIntent::RunStarted(RunStarted {
            run_id: run_id.clone(),
        }));
    }
    model.apply(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
        run_id: awaiting_id.clone(),
    }));

    model.apply(ConversationIntent::RunCompleted(RunCompleted {
        run_id: complete_id.clone(),
    }));
    model.apply(ConversationIntent::RunFailed(RunFailed {
        run_id: failed_id.clone(),
    }));
    let changes = model.apply(ConversationIntent::RunCompleted(RunCompleted {
        run_id: awaiting_id.clone(),
    }));

    assert_eq!(
        model.agent_run(&complete_id).unwrap().phase(),
        AgentRunPhase::Completed
    );
    assert_eq!(
        model.agent_run(&failed_id).unwrap().phase(),
        AgentRunPhase::Failed
    );
    assert!(changes.is_empty());
    assert_eq!(
        model.agent_run(&awaiting_id).unwrap().phase(),
        AgentRunPhase::AwaitingUser
    );
}

#[test]
fn terminal_run_rejects_resume() {
    let mut model = ConversationModel::default();
    let run_id = UiRunId::from("run-1");

    for intent in [
        ConversationIntent::RunStarted(RunStarted {
            run_id: run_id.clone(),
        }),
        ConversationIntent::RunCancelling(RunCancelling {
            run_id: run_id.clone(),
        }),
        ConversationIntent::RunCancelled(RunCancelled {
            run_id: run_id.clone(),
        }),
    ] {
        model.apply(intent);
    }

    let changes = model.apply(ConversationIntent::RunResumed(RunResumed {
        run_id: run_id.clone(),
    }));

    assert!(changes.is_empty());
    assert_eq!(
        model.agent_run(&run_id).unwrap().phase(),
        AgentRunPhase::Cancelled
    );
}

use sdk::{
    InteractionCancelReason, InteractionReply, InteractionReplyError, InteractionRequestId, RunId,
    UserAnswer, UserQuestion,
};

use crate::application::interaction::coordinator::{CoordinationError, InteractionCoordinator};
use crate::application::interaction::port::{validate_reply, InteractionBridge, InteractionPort};

#[test]
fn p6_5_interaction_mailbox_and_completion_have_single_application_owner() {
    let coordinator = include_str!("coordinator.rs");
    let main_adapter = include_str!("../loop_engine/chat/main_run_port.rs");
    let sub_adapter = include_str!("../run/derived/loop_run.rs");
    let engine = include_str!("../loop_engine/engine.rs");

    assert!(coordinator.contains("pub fn poll_mailbox"));
    assert!(coordinator.contains("pub(crate) async fn complete_tool_interaction"));
    for adapter in [main_adapter, sub_adapter] {
        assert!(!adapter.contains("async fn poll_interaction("));
        assert!(!adapter.contains("async fn finish_interaction_work("));
    }
    assert!(!engine.contains(".finish_interaction_work("));
}
use crate::domain::agent_run::{
    InteractionContinuation, ModelInvocation, Run, RunSpec, RunStatus, RunTransition,
};

/// Advance a fresh Run into ExecutingTools so `begin_interaction` with
/// `CompleteToolCall`/`ContinueAfterHardPause` is allowed.
fn run_in_executing_tools() -> Run {
    let mut run = Run::new(RunSpec::main(), None);
    // Created → DrainingInput
    run.transition(RunTransition::StartDraining).unwrap();
    // DrainingInput → PreparingContext
    run.transition(RunTransition::DrainInputs).unwrap();
    // PreparingContext → InvokingModel
    run.transition(RunTransition::ContextPrepared).unwrap();
    // Need an active step with invocation for ModelInvoked
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("resp"))
        .unwrap();
    // InvokingModel → ApplyingResponse
    run.transition(RunTransition::ModelInvoked).unwrap();
    // ApplyingResponse → AwaitingToolApproval
    run.transition(RunTransition::ResponseWithTools).unwrap();
    // AwaitingToolApproval → ExecutingTools
    run.transition(RunTransition::ToolsApproved).unwrap();
    run
}

// ── Helpers ──

fn question_body() -> (sdk::InteractionRequestBody, InteractionContinuation) {
    (
        sdk::InteractionRequestBody::UserQuestions(vec![UserQuestion {
            prompt: "继续？".to_string(),
            options: vec!["是".to_string()],
            allow_multi: false,
        }]),
        InteractionContinuation::CompleteToolCall(sdk::ids::ToolCallId::from_legacy_or_new(
            "test-call",
        )),
    )
}

// ── Begin tests ──

#[tokio::test]
async fn coordinator_begin_user_questions() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (rid, _receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id,
        body,
        continuation,
    )
    .unwrap();

    assert_eq!(rid, request_id);
    assert!(run.pending_interaction().is_some());
    assert!(port.contains(&request_id));
}

// ── Atomic begin: compensation tests (Task 4.1) ──

#[tokio::test]
async fn coordinator_begin_fails_when_port_unavailable() {
    let mut run = run_in_executing_tools();
    let port = crate::application::interaction::port::UnavailableInteractionPort;
    let (body, continuation) = question_body();

    let err = InteractionCoordinator::begin(
        &mut run,
        &port,
        InteractionRequestId::new_v7(),
        RunId::new_v7(),
        body,
        continuation,
    )
    .unwrap_err();

    assert_eq!(err, CoordinationError::Unavailable);
    // Run must NOT be left in AwaitingUser — no state change happened
    assert!(run.pending_interaction().is_none());
    assert_ne!(run.status(), RunStatus::AwaitingUser);
}

#[tokio::test]
async fn coordinator_begin_fails_on_duplicate_registration() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    // First begin succeeds
    let (rid1, _receiver1) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    )
    .unwrap();
    assert_eq!(rid1, request_id);

    // Completing this first interaction to free the run
    port.cancel(&request_id, InteractionCancelReason::UserCancelled);
    InteractionCoordinator::cancel(&mut run, &request_id).unwrap();

    // Start a new begin with the SAME request_id on the SAME port → duplicate
    let mut run2 = run_in_executing_tools();
    let request2_id = InteractionRequestId::new_v7();
    let (body2, continuation2) = question_body();

    // Register first manually, then try begin with same id on same port
    let req = sdk::InteractionRequest {
        id: request2_id.clone(),
        run_id: RunId::new_v7(),
        body: body2.clone(),
    };
    let _pre_reg = port.register(req).unwrap();

    let err = InteractionCoordinator::begin(
        &mut run2,
        &port,
        request2_id.clone(),
        RunId::new_v7(),
        body2,
        continuation2,
    )
    .unwrap_err();

    assert_eq!(err, CoordinationError::AlreadyRegistered);
    // Run must NOT be left in AwaitingUser
    assert!(run2.pending_interaction().is_none());
}

#[tokio::test]
async fn coordinator_begin_fails_on_domain_rejection_with_compensation() {
    // Use a Run that isn't in ExecutingTools — domain will reject
    let mut run = Run::new(RunSpec::main(), None);
    assert_eq!(run.status(), RunStatus::Created);

    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();

    let err = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        RunId::new_v7(),
        body,
        continuation,
    )
    .unwrap_err();

    assert!(matches!(err, CoordinationError::RunError(_)));
    // Run must NOT be left in AwaitingUser
    assert!(run.pending_interaction().is_none());
    assert_eq!(run.status(), RunStatus::Created);
    // Port must be cleaned up — request_id should not be pending
    assert!(!port.contains(&request_id));
}

// ── Shared validate_reply via the public fn (Task 4.4) ──

#[test]
fn shared_validate_user_questions_accepts_matching_reply() {
    let body = sdk::InteractionRequestBody::UserQuestions(vec![UserQuestion {
        prompt: "Q".into(),
        options: vec![],
        allow_multi: false,
    }]);
    let reply = InteractionReply::UserQuestions(vec![UserAnswer("A".into())]);
    assert!(validate_reply(&body, &reply).is_ok());
}

#[test]
fn shared_validate_user_questions_rejects_wrong_count() {
    let body = sdk::InteractionRequestBody::UserQuestions(vec![
        UserQuestion {
            prompt: "Q1".into(),
            options: vec![],
            allow_multi: false,
        },
        UserQuestion {
            prompt: "Q2".into(),
            options: vec![],
            allow_multi: false,
        },
    ]);
    let reply = InteractionReply::UserQuestions(vec![UserAnswer("A1".into())]);
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::AnswerCountMismatch
    );
}

#[test]
fn shared_validate_rejects_variant_mismatch() {
    let body = sdk::InteractionRequestBody::UserQuestions(vec![UserQuestion {
        prompt: "Q".into(),
        options: vec![],
        allow_multi: false,
    }]);
    let reply = InteractionReply::HardPauseContinue;
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::VariantMismatch
    );
}

#[test]
fn shared_validate_tool_approval_accepts_matching() {
    let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
        tool_name: "bash".into(),
        args_summary: "...".into(),
        risk_level: sdk::RiskLevel::Low,
    });
    let reply = InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve);
    assert!(validate_reply(&body, &reply).is_ok());
}

#[test]
fn shared_validate_tool_approval_rejects_variant_mismatch() {
    let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
        tool_name: "bash".into(),
        args_summary: "...".into(),
        risk_level: sdk::RiskLevel::Low,
    });
    let reply = InteractionReply::PlanApproval(sdk::ApprovalDecision::Approve);
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::VariantMismatch
    );
}

#[test]
fn shared_validate_plan_approval_accepts_matching() {
    let body = sdk::InteractionRequestBody::PlanApproval(sdk::PlanApprovalPrompt {
        plan_title: "Plan".into(),
        steps: vec![],
    });
    let reply = InteractionReply::PlanApproval(sdk::ApprovalDecision::Approve);
    assert!(validate_reply(&body, &reply).is_ok());
}

#[test]
fn shared_validate_hard_pause_accepts_continue() {
    let body = sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
        reason: "stuck".into(),
        recent_actions: vec![],
    });
    let reply = InteractionReply::HardPauseContinue;
    assert!(validate_reply(&body, &reply).is_ok());
}

#[test]
fn shared_validate_hard_pause_rejects_variant_mismatch() {
    let body = sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
        reason: "stuck".into(),
        recent_actions: vec![],
    });
    let reply = InteractionReply::UserQuestions(vec![UserAnswer("A".into())]);
    assert_eq!(
        validate_reply(&body, &reply).unwrap_err(),
        InteractionReplyError::VariantMismatch
    );
}

// ── Complete reply round-trip ──

#[tokio::test]
async fn coordinator_full_roundtrip_reply() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (rid, receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id,
        body.clone(),
        continuation,
    )
    .unwrap();

    // Reply on the port (simulating TUI)
    port.reply(
        &request_id,
        InteractionReply::UserQuestions(vec![UserAnswer("是".into())]),
    );

    let completion = InteractionCoordinator::wait(receiver).await.unwrap();
    match &completion {
        crate::application::interaction::port::InteractionCompletion::Replied(reply) => {
            let result =
                InteractionCoordinator::complete_reply(&mut run, &rid, &body, reply).unwrap();
            assert_eq!(
                result,
                InteractionContinuation::CompleteToolCall(
                    sdk::ids::ToolCallId::from_legacy_or_new("test-call")
                )
            );
        }
        _ => panic!("expected Replied"),
    }
    assert!(run.pending_interaction().is_none());
}

#[tokio::test]
async fn coordinator_begin_then_cancel() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (rid, _receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id,
        body.clone(),
        continuation,
    )
    .unwrap();

    // Cancel on the port
    port.cancel(&request_id, InteractionCancelReason::UserCancelled);

    let result = InteractionCoordinator::cancel(&mut run, &rid).unwrap();
    assert_eq!(
        result,
        InteractionContinuation::CompleteToolCall(sdk::ids::ToolCallId::from_legacy_or_new(
            "test-call"
        ))
    );
    assert!(run.pending_interaction().is_none());
}

// ── Cancel-and-drain (Task 4.2): complete disconnect handling ──

#[test]
fn coordinator_cancel_and_drain_transitions_run_to_cancelling() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (_rid, _receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id.clone(),
        body,
        continuation,
    )
    .unwrap();

    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(run.pending_interaction().is_some());
    assert!(port.contains(&request_id));

    // Cancel and drain — full disconnect
    InteractionCoordinator::cancel_and_drain(
        &mut run,
        &port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    )
    .unwrap();

    // Run must be in Cancelling, with no pending interaction
    assert_eq!(run.status(), RunStatus::Cancelling);
    assert!(run.pending_interaction().is_none());
    // Port must be drained
    assert!(!port.contains(&request_id));
}

#[test]
fn coordinator_cancel_and_drain_idempotent_on_terminal_run() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (_rid, _receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id.clone(),
        body,
        continuation,
    )
    .unwrap();

    // First cancel
    InteractionCoordinator::cancel_and_drain(
        &mut run,
        &port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    )
    .unwrap();
    assert_eq!(run.status(), RunStatus::Cancelling);

    // Complete the cancellation to make it terminal
    run.finish_cancellation().unwrap();
    assert!(run.status().is_terminal());

    // Second cancel_and_drain on already-terminal Run — must not panic
    let result = InteractionCoordinator::cancel_and_drain(
        &mut run,
        &port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    );
    assert!(result.is_ok());
    assert!(run.status().is_terminal());
}

#[test]
fn coordinator_cancel_and_drain_does_not_leave_awaiting_user_without_pending() {
    let mut run = run_in_executing_tools();
    let port = InteractionBridge::new();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (_rid, _receiver) = InteractionCoordinator::begin(
        &mut run,
        &port,
        request_id.clone(),
        run_id.clone(),
        body,
        continuation,
    )
    .unwrap();

    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(run.pending_interaction().is_some());

    InteractionCoordinator::cancel_and_drain(
        &mut run,
        &port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    )
    .unwrap();

    // MUST NOT be AwaitingUser with no pending (the critical bug)
    if run.status() == RunStatus::AwaitingUser {
        assert!(
            run.pending_interaction().is_some(),
            "AwaitingUser must have pending_interaction"
        );
    }
    // Should be Cancelling
    assert_eq!(run.status(), RunStatus::Cancelling);
    assert!(run.pending_interaction().is_none());
}

// ── Parent adapter / channel drop via coordinator (Task 4.5) ──

#[tokio::test]
async fn parent_adapter_drop_via_coordinator_does_not_hang_run() {
    use std::sync::Arc;

    let parent_port: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    // ParentMediated reuses the parent port directly (no wrapper adapter).

    let mut run = run_in_executing_tools();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    let (_rid, receiver) = InteractionCoordinator::begin(
        &mut run,
        &*parent_port,
        request_id.clone(),
        run_id.clone(),
        body,
        continuation,
    )
    .unwrap();

    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(run.pending_interaction().is_some());

    // Simulate parent dropping — drain the parent port
    parent_port.drain_run(&run_id, InteractionCancelReason::RunCancelled);

    // The child's receiver should resolve with Cancelled
    let completion = InteractionCoordinator::wait(receiver).await.unwrap();
    assert_eq!(
        completion,
        crate::application::interaction::port::InteractionCompletion::Cancelled(
            InteractionCancelReason::RunCancelled
        )
    );

    // Now cancel_and_drain via coordinator to clean up Run
    InteractionCoordinator::cancel_and_drain(
        &mut run,
        &*parent_port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    )
    .unwrap();

    assert_eq!(run.status(), RunStatus::Cancelling);
    assert!(run.pending_interaction().is_none());
}

#[tokio::test]
async fn parent_adapter_drop_does_not_hang_run_with_full_coordinator_api() {
    use std::sync::Arc;

    let parent_port: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    // ParentMediated reuses the parent port directly (no wrapper adapter).

    let mut run = run_in_executing_tools();
    let (body, continuation) = question_body();
    let request_id = InteractionRequestId::new_v7();
    let run_id = RunId::new_v7();

    // Begin through coordinator with parent port
    let (_rid, receiver) = InteractionCoordinator::begin(
        &mut run,
        &*parent_port,
        request_id.clone(),
        run_id.clone(),
        body,
        continuation,
    )
    .unwrap();

    assert_eq!(run.status(), RunStatus::AwaitingUser);

    // Drop the parent port entirely — simulates parent process exit while
    // the child's coordinator registration is still active.
    // With ParentMediated directly reusing the parent port, dropping the
    // last Arc should trigger WaiterDropped or resolve without hanging.
    drop(parent_port);

    // The receiver should return WaiterDropped since the port is gone
    let result = InteractionCoordinator::wait(receiver).await;
    assert!(
        matches!(result, Err(CoordinationError::WaiterDropped) | Ok(_)),
        "waiter should resolve or error, not hang"
    );

    // Run should still be recoverable — cancel_and_drain via a new port
    let port = InteractionBridge::new();
    let run_id = RunId::new_v7();
    InteractionCoordinator::cancel_and_drain(
        &mut run,
        &port,
        &run_id,
        InteractionCancelReason::RunCancelled,
    )
    .unwrap();

    assert!(matches!(
        run.status(),
        RunStatus::Cancelling | RunStatus::Cancelled
    ));
    assert!(run.pending_interaction().is_none());
}

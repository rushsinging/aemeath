use super::*;
use sdk::InteractionRequestId;
use std::time::Duration;

fn run() -> Run {
    Run::new(RunSpec::new("main", Duration::ZERO), None)
}

fn tool_continuation(provider_id: &str) -> InteractionContinuation {
    InteractionContinuation::CompleteToolCall(sdk::ids::ToolCallId::from_legacy_or_new(provider_id))
}

#[test]
fn pending_interaction_enters_awaiting_user_and_emits_request_identity() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    let request_id = InteractionRequestId::new_v7();
    let continuation = tool_continuation("call-1");

    run.begin_interaction(request_id.clone(), continuation.clone())
        .unwrap();

    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert_eq!(
        run.pending_interaction(),
        Some(&PendingInteraction {
            request_id: request_id.clone(),
            continuation,
        })
    );
    assert!(run.events().iter().any(|event| matches!(
        event,
        RunDomainEvent::AwaitingUser {
            request_id: emitted,
            ..
        } if emitted == &request_id
    )));
}

#[test]
fn run_rejects_second_pending_interaction_without_overwriting_first() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    let first = InteractionRequestId::new_v7();
    let second = InteractionRequestId::new_v7();
    run.begin_interaction(first.clone(), tool_continuation("call-1"))
        .unwrap();

    assert_eq!(
        run.begin_interaction(second, tool_continuation("call-2")),
        Err(RunTransitionError::InteractionAlreadyPending(first.clone()))
    );
    assert_eq!(
        run.pending_interaction().map(|pending| &pending.request_id),
        Some(&first)
    );
}

#[test]
fn completing_interaction_requires_matching_id_and_clears_exactly_once() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    let request_id = InteractionRequestId::new_v7();
    let stale_id = InteractionRequestId::new_v7();
    let continuation = InteractionContinuation::ContinueAfterHardPause;
    run.begin_interaction(request_id.clone(), continuation.clone())
        .unwrap();

    assert_eq!(
        run.complete_interaction(&stale_id),
        Err(RunTransitionError::InteractionRequestMismatch {
            expected: request_id.clone(),
            received: stale_id,
        })
    );
    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(run.pending_interaction().is_some());

    assert_eq!(run.complete_interaction(&request_id).unwrap(), continuation);
    assert_eq!(run.status(), RunStatus::ExecutingTools);
    assert!(run.pending_interaction().is_none());
    assert_eq!(
        run.complete_interaction(&request_id),
        Err(RunTransitionError::NoPendingInteraction)
    );
}

#[test]
fn cancelling_interaction_clears_pending_without_emitting_resumed() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    let request_id = InteractionRequestId::new_v7();
    let continuation = tool_continuation("call-cancel");
    run.begin_interaction(request_id.clone(), continuation.clone())
        .unwrap();
    run.drain_events();

    assert_eq!(run.cancel_interaction(&request_id).unwrap(), continuation);

    assert_eq!(run.status(), RunStatus::AwaitingUser);
    assert!(run.pending_interaction().is_none());
    assert!(!run
        .events()
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Resumed { .. })));
}

#[test]
fn interaction_continuation_exhaustively_restores_its_origin_phase() {
    let call_id = sdk::ids::ToolCallId::from_legacy_or_new("call-1");
    let cases = [
        (
            RunStatus::ExecutingTools,
            InteractionContinuation::CompleteToolCall(call_id.clone()),
            RunStatus::ExecutingTools,
        ),
        (
            RunStatus::AwaitingToolApproval,
            InteractionContinuation::ContinueToolApproval(call_id),
            RunStatus::AwaitingToolApproval,
        ),
        (
            RunStatus::ApplyingResponse,
            InteractionContinuation::ContinuePlanApproval,
            RunStatus::PreparingContext,
        ),
        (
            RunStatus::ExecutingTools,
            InteractionContinuation::ContinueAfterHardPause,
            RunStatus::ExecutingTools,
        ),
    ];

    for (initial, continuation, expected) in cases {
        let mut run = run_at_status(initial);
        let request_id = InteractionRequestId::new_v7();
        run.begin_interaction(request_id.clone(), continuation)
            .unwrap();
        run.complete_interaction(&request_id).unwrap();
        assert_eq!(run.status(), expected);
    }
}

#[test]
fn run_control_clears_pending_interaction_before_terminal_or_step_finalization() {
    let mut terminated = run_at_status(RunStatus::ExecutingTools);
    let termination_request = InteractionRequestId::new_v7();
    terminated
        .begin_interaction(
            termination_request,
            InteractionContinuation::ContinueAfterHardPause,
        )
        .unwrap();
    assert_eq!(
        terminated.request_termination(
            sdk::RunTerminationReason::UserExit,
            sdk::ControlDeadline::from_unix_millis(10),
        ),
        RunTerminationRequest::Accepted
    );
    assert!(terminated.pending_interaction().is_none());

    let mut cancelled = run_at_status(RunStatus::ExecutingTools);
    let step_id = cancelled.active_step_id().unwrap();
    cancelled
        .begin_interaction(InteractionRequestId::new_v7(), tool_continuation("call-2"))
        .unwrap();
    assert_eq!(
        cancelled.request_step_cancellation(&step_id),
        RunStepCancellationRequest::Accepted
    );
    assert!(cancelled.pending_interaction().is_none());
}

#[test]
fn preparing_context_creates_step_before_compaction_and_can_cancel_it() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();

    let step_id = run.begin_step().unwrap();

    assert_eq!(run.status(), RunStatus::PreparingContext);
    assert_eq!(run.active_step_id(), Some(step_id.clone()));
    run.transition(RunTransition::BeginCompaction).unwrap();
    assert_eq!(run.status(), RunStatus::Compacting);
    assert_eq!(
        run.request_step_cancellation(&step_id),
        RunStepCancellationRequest::Accepted
    );
    run.begin_step_finalization(&step_id).unwrap();
    run.finish_cancelled_step(&step_id).unwrap();

    assert_eq!(run.status(), RunStatus::DrainingInput);
    assert_eq!(run.steps()[0].status(), RunStepStatus::Cancelled);
}

#[test]
fn new_control_path_drains_before_work_and_after_cancelled_step() {
    let mut run = run();

    run.start_draining().unwrap();
    assert_eq!(run.status(), RunStatus::DrainingInput);
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    assert_eq!(run.status(), RunStatus::PreparingContext);
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();

    assert_eq!(
        run.request_step_cancellation(&step_id),
        RunStepCancellationRequest::Accepted
    );
    run.begin_step_finalization(&step_id).unwrap();
    run.finish_cancelled_step(&step_id).unwrap();

    assert_eq!(run.steps()[0].status(), RunStepStatus::Cancelled);
    assert_eq!(run.status(), RunStatus::DrainingInput);
    run.apply_drain_decision(DrainDecision::EmptyAndSealed, None)
        .unwrap();
    assert_eq!(run.status(), RunStatus::Completed);
}

#[test]
fn scenario_cancelled_step_drains_input_then_starts_fresh_step() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let cancelled = run.begin_step().unwrap();
    run.request_step_cancellation(&cancelled);
    run.begin_step_finalization(&cancelled).unwrap();
    run.finish_cancelled_step(&cancelled).unwrap();

    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let next = run.begin_step().unwrap();

    assert_ne!(cancelled, next);
    assert_eq!(run.steps()[0].status(), RunStepStatus::Cancelled);
    assert_eq!(run.steps()[1].status(), RunStepStatus::Invoking);
}

#[test]
fn scenario_terminate_discards_controlled_step_and_closes_run() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    let deadline = sdk::ControlDeadline::from_unix_millis(10_000);
    run.request_step_cancellation(&step_id);

    run.request_termination(sdk::RunTerminationReason::SessionShutdown, deadline);
    run.finish_termination().unwrap();

    assert_eq!(run.status(), RunStatus::Terminated);
    assert_eq!(
        run.steps()[0].status(),
        RunStepStatus::CancellationUnconfirmed
    );
    assert!(run.events().iter().any(|event| matches!(
        event,
        RunDomainEvent::Terminated {
            reason: sdk::RunTerminationReason::SessionShutdown,
            ..
        }
    )));
}
#[test]
fn internal_continuation_leaves_drain_for_preparing_context() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::InternalContinuation, None)
        .unwrap();

    assert_eq!(run.status(), RunStatus::PreparingContext);
}

#[test]
fn termination_preempts_step_cancellation_and_is_idempotent() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    let deadline = sdk::ControlDeadline::from_unix_millis(1234);

    assert_eq!(
        run.request_step_cancellation(&step_id),
        RunStepCancellationRequest::Accepted
    );
    assert_eq!(
        run.request_termination(sdk::RunTerminationReason::UserExit, deadline),
        RunTerminationRequest::Accepted
    );
    assert_eq!(
        run.request_termination(sdk::RunTerminationReason::UserExit, deadline),
        RunTerminationRequest::AlreadyTerminating
    );
    assert_eq!(run.status(), RunStatus::Terminating);
    assert_eq!(
        run.request_step_cancellation(&step_id),
        RunStepCancellationRequest::RunTerminating
    );

    run.finish_termination().unwrap();
    assert_eq!(run.status(), RunStatus::Terminated);
    assert!(run.is_terminal());
}

#[test]
fn cancellation_deadline_can_close_step_as_unconfirmed_then_drain() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    run.request_step_cancellation(&step_id);
    run.begin_step_finalization(&step_id).unwrap();

    run.finish_unconfirmed_step(&step_id).unwrap();

    assert_eq!(
        run.steps()[0].status(),
        RunStepStatus::CancellationUnconfirmed
    );
    assert_eq!(run.status(), RunStatus::DrainingInput);
}
#[test]
fn run_follows_the_happy_path_to_completed() {
    let mut run = run();

    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    run.transition(RunTransition::ContinueAfterResponse)
        .unwrap();
    run.complete_step(&step_id).unwrap();
    run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some("final answer"))
        .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(run.is_terminal());
    assert!(matches!(
        run.events().last(),
        Some(RunDomainEvent::Completed { result, .. }) if result == "final answer"
    ));
}

#[test]
fn every_state_change_emits_transitioned_with_reason() {
    let mut run = run();

    run.start_draining().unwrap();
    run.fail("provider failed").unwrap();

    let transitions: Vec<_> = run
        .events()
        .iter()
        .filter_map(|event| match event {
            RunDomainEvent::Transitioned {
                from, to, reason, ..
            } => Some((*from, *to, *reason)),
            _ => None,
        })
        .collect();

    assert_eq!(
        transitions,
        vec![
            (
                RunStatus::Created,
                RunStatus::DrainingInput,
                RunTransitionReason::DrainStarted,
            ),
            (
                RunStatus::DrainingInput,
                RunStatus::Failed,
                RunTransitionReason::Failed,
            ),
        ]
    );
}

#[test]
fn cancellation_and_completion_use_the_same_transition_event() {
    let mut cancelled = run();
    cancelled.start_draining().unwrap();
    cancelled.request_cancellation();
    cancelled.finish_cancellation().unwrap();

    assert!(cancelled.events().iter().any(|event| matches!(
        event,
        RunDomainEvent::Transitioned {
            from: RunStatus::DrainingInput,
            to: RunStatus::Cancelling,
            reason: RunTransitionReason::InterruptRequested,
            ..
        }
    )));
    assert!(cancelled.events().iter().any(|event| matches!(
        event,
        RunDomainEvent::Transitioned {
            from: RunStatus::Cancelling,
            to: RunStatus::Cancelled,
            reason: RunTransitionReason::CancellationFinished,
            ..
        }
    )));
}

#[test]
fn rejected_transition_does_not_emit_transitioned_event() {
    let mut run = run();

    let _ = run.transition(RunTransition::ModelInvoked);

    assert!(!run
        .events()
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Transitioned { .. })));
}
#[test]
fn run_rejects_illegal_transition_without_mutating_status() {
    let mut run = run();

    let error = run.transition(RunTransition::ModelInvoked).unwrap_err();

    assert_eq!(run.status(), RunStatus::Created);
    assert_eq!(
        error,
        RunTransitionError::IllegalTransition {
            from: RunStatus::Created,
            transition: RunTransition::ModelInvoked,
        }
    );
}

#[test]
fn cancellation_is_two_phase_and_idempotent() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();

    assert_eq!(run.request_cancellation(), RunCancellationRequest::Accepted);
    assert_eq!(run.status(), RunStatus::Cancelling);
    assert_eq!(
        run.request_cancellation(),
        RunCancellationRequest::AlreadyCancelling
    );

    run.finish_cancellation().unwrap();

    assert_eq!(run.status(), RunStatus::Cancelled);
    assert_eq!(
        run.request_cancellation(),
        RunCancellationRequest::AlreadyTerminal
    );
    let lifecycle: Vec<_> = run
        .events()
        .iter()
        .filter(|event| !matches!(event, RunDomainEvent::Transitioned { .. }))
        .cloned()
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            RunDomainEvent::Started {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
            RunDomainEvent::DrainingInput {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
            RunDomainEvent::CancellationRequested {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
            RunDomainEvent::Cancelled {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
        ]
    );
}

#[test]
fn cancelling_run_rejects_new_work() {
    let mut run = run();
    run.start_draining().unwrap();
    run.request_cancellation();

    assert!(matches!(
        run.begin_step(),
        Err(RunTransitionError::RunNotActive(RunStatus::Cancelling))
    ));
    assert!(matches!(
        run.transition(RunTransition::BeginCompaction),
        Err(RunTransitionError::IllegalTransition {
            from: RunStatus::Cancelling,
            transition: RunTransition::BeginCompaction,
        })
    ));
}

#[test]
fn cancellation_closes_the_active_step_and_rejects_late_completion() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();

    run.request_cancellation();
    run.finish_cancellation().unwrap();

    assert_eq!(run.steps()[0].status(), RunStepStatus::Cancelled);
    assert!(matches!(
        run.complete_step(&step_id),
        Err(RunTransitionError::RunNotActive(RunStatus::Cancelled))
    ));
}

#[test]
fn terminal_run_rejects_new_steps() {
    let mut run = run();
    run.start_draining().unwrap();
    run.fail("boom").unwrap();

    assert!(matches!(
        run.begin_step(),
        Err(RunTransitionError::RunNotActive(RunStatus::Failed))
    ));
}

#[test]
fn parent_identity_is_carried_by_every_domain_event() {
    let parent = RunId::new_v7();
    let mut run = Run::new(
        RunSpec::new("sub", Duration::from_secs(30)),
        Some(parent.clone()),
    );

    run.start_draining().unwrap();
    run.fail("failed").unwrap();

    assert_eq!(run.parent_id(), Some(&parent));
    assert!(run
        .events()
        .iter()
        .all(|event| event.parent_run_id() == Some(&parent)));
}

const ALL_RUN_STATUSES: [RunStatus; 17] = [
    RunStatus::Created,
    RunStatus::DrainingInput,
    RunStatus::PreparingContext,
    RunStatus::InvokingModel,
    RunStatus::ApplyingResponse,
    RunStatus::AwaitingToolApproval,
    RunStatus::ExecutingTools,
    RunStatus::AwaitingUser,
    RunStatus::Compacting,
    RunStatus::CancellingStep,
    RunStatus::FinalizingStep,
    RunStatus::Cancelling,
    RunStatus::Terminating,
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Cancelled,
    RunStatus::Terminated,
];

const ALL_RUN_TRANSITIONS: [RunTransition; 19] = [
    RunTransition::StartDraining,
    RunTransition::DrainInputs,
    RunTransition::DrainInternalContinuation,
    RunTransition::DrainEmptyAndSealed,
    RunTransition::BeginCompaction,
    RunTransition::CompactionCompleted,
    RunTransition::ContextPrepared,
    RunTransition::RetryModel,
    RunTransition::ModelContextExceeded,
    RunTransition::ModelInvoked,
    RunTransition::ResponseWithTools,
    RunTransition::ContinueAfterResponse,
    RunTransition::ToolsApproved,
    RunTransition::AwaitUser,
    RunTransition::UserResumed,
    RunTransition::ToolsCompleted,
    RunTransition::StepCancelled,
    RunTransition::TerminationFinished,
    RunTransition::CancellationFinished,
];

fn invoke_to_applying(run: &mut Run) -> RunStepId {
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    step_id
}

fn run_at_status(status: RunStatus) -> Run {
    let mut run = run();
    if status == RunStatus::Created {
        return run;
    }
    if status == RunStatus::DrainingInput {
        run.start_draining().unwrap();
        return run;
    }

    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    match status {
        RunStatus::Created | RunStatus::DrainingInput => unreachable!(),
        RunStatus::PreparingContext => {}
        RunStatus::Compacting => {
            run.transition(RunTransition::BeginCompaction).unwrap();
        }
        RunStatus::InvokingModel => {
            run.transition(RunTransition::ContextPrepared).unwrap();
        }
        RunStatus::ApplyingResponse => {
            invoke_to_applying(&mut run);
        }
        RunStatus::AwaitingToolApproval => {
            invoke_to_applying(&mut run);
            run.transition(RunTransition::ResponseWithTools).unwrap();
        }
        RunStatus::ExecutingTools => {
            invoke_to_applying(&mut run);
            run.transition(RunTransition::ResponseWithTools).unwrap();
            run.transition(RunTransition::ToolsApproved).unwrap();
        }
        RunStatus::AwaitingUser => {
            invoke_to_applying(&mut run);
            run.transition(RunTransition::ResponseWithTools).unwrap();
            run.transition(RunTransition::AwaitUser).unwrap();
        }
        RunStatus::CancellingStep => {
            run.transition(RunTransition::ContextPrepared).unwrap();
            let step_id = run.begin_step().unwrap();
            run.request_step_cancellation(&step_id);
        }
        RunStatus::FinalizingStep => {
            run.transition(RunTransition::ContextPrepared).unwrap();
            let step_id = run.begin_step().unwrap();
            run.request_step_cancellation(&step_id);
            run.begin_step_finalization(&step_id).unwrap();
        }
        RunStatus::Cancelling => {
            assert_eq!(run.request_cancellation(), RunCancellationRequest::Accepted);
        }
        RunStatus::Terminating => {
            run.request_termination(
                sdk::RunTerminationReason::UserExit,
                sdk::ControlDeadline::from_unix_millis(1),
            );
        }
        RunStatus::Completed => {
            let step_id = invoke_to_applying(&mut run);
            run.transition(RunTransition::ContinueAfterResponse)
                .unwrap();
            run.complete_step(&step_id).unwrap();
            run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some("done"))
                .unwrap();
        }
        RunStatus::Terminated => {
            run.request_termination(
                sdk::RunTerminationReason::UserExit,
                sdk::ControlDeadline::from_unix_millis(1),
            );
            run.finish_termination().unwrap();
        }
        RunStatus::Failed => {
            run.fail("failed").unwrap();
        }
        RunStatus::Cancelled => {
            assert_eq!(run.request_cancellation(), RunCancellationRequest::Accepted);
            run.finish_cancellation().unwrap();
        }
    }
    assert_eq!(run.status(), status);
    run
}

fn expected_transition(from: RunStatus, transition: RunTransition) -> Option<RunStatus> {
    match (from, transition) {
        (RunStatus::Created, RunTransition::StartDraining) => Some(RunStatus::DrainingInput),
        (RunStatus::DrainingInput, RunTransition::DrainInputs)
        | (RunStatus::DrainingInput, RunTransition::DrainInternalContinuation) => {
            Some(RunStatus::PreparingContext)
        }
        (RunStatus::DrainingInput, RunTransition::DrainEmptyAndSealed) => {
            Some(RunStatus::Completed)
        }
        (RunStatus::PreparingContext, RunTransition::BeginCompaction) => {
            Some(RunStatus::Compacting)
        }
        (RunStatus::Compacting, RunTransition::CompactionCompleted) => {
            Some(RunStatus::PreparingContext)
        }
        (RunStatus::PreparingContext, RunTransition::ContextPrepared) => {
            Some(RunStatus::InvokingModel)
        }
        (RunStatus::InvokingModel, RunTransition::RetryModel) => Some(RunStatus::InvokingModel),
        (RunStatus::InvokingModel, RunTransition::ModelContextExceeded) => {
            Some(RunStatus::Compacting)
        }
        (RunStatus::InvokingModel, RunTransition::ModelInvoked) => {
            Some(RunStatus::ApplyingResponse)
        }
        (RunStatus::ApplyingResponse, RunTransition::ResponseWithTools) => {
            Some(RunStatus::AwaitingToolApproval)
        }
        (RunStatus::ApplyingResponse, RunTransition::ContinueAfterResponse) => {
            Some(RunStatus::DrainingInput)
        }
        (RunStatus::AwaitingToolApproval, RunTransition::ToolsApproved) => {
            Some(RunStatus::ExecutingTools)
        }
        (RunStatus::AwaitingToolApproval, RunTransition::AwaitUser)
        | (RunStatus::ExecutingTools, RunTransition::AwaitUser) => Some(RunStatus::AwaitingUser),
        (RunStatus::AwaitingUser, RunTransition::UserResumed) => Some(RunStatus::DrainingInput),
        (RunStatus::ExecutingTools, RunTransition::ToolsCompleted) => {
            Some(RunStatus::DrainingInput)
        }
        (RunStatus::FinalizingStep, RunTransition::StepCancelled) => Some(RunStatus::DrainingInput),
        (RunStatus::Terminating, RunTransition::TerminationFinished) => Some(RunStatus::Terminated),
        (RunStatus::Cancelling, RunTransition::CancellationFinished) => Some(RunStatus::Cancelled),
        _ => None,
    }
}

#[test]
fn run_transition_matrix_exhaustively_accepts_only_documented_edges() {
    for from in ALL_RUN_STATUSES {
        for transition in ALL_RUN_TRANSITIONS {
            let mut run = run_at_status(from);
            if from == RunStatus::InvokingModel && transition == RunTransition::ModelInvoked {
                let step_id = run.begin_step().unwrap();
                run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
                    .unwrap();
            }
            let result = run.transition(transition);
            match expected_transition(from, transition) {
                Some(expected) => assert_eq!(result, Ok(expected), "{from:?} --{transition:?}"),
                None => {
                    assert_eq!(
                        result,
                        Err(RunTransitionError::IllegalTransition { from, transition }),
                        "{from:?} --{transition:?}"
                    );
                    assert_eq!(run.status(), from, "非法迁移不得修改状态");
                }
            }
        }
    }
}

fn tool_call(provider_id: &str) -> crate::domain::agent_run::ToolCall {
    crate::domain::agent_run::ToolCall {
        id: sdk::ids::ToolCallId::from_legacy_or_new(provider_id),
        provider_id: provider_id.to_string(),
        name: "Read".to_string(),
        index: 0,
        input: serde_json::json!({"file_path": "README.md"}),
    }
}

#[test]
fn model_invoked_requires_recorded_invocation_on_active_step() {
    let mut run = run_at_status(RunStatus::InvokingModel);

    assert_eq!(
        run.transition(RunTransition::ModelInvoked),
        Err(RunTransitionError::StepIncomplete)
    );
    let step_id = run.begin_step().unwrap();
    assert_eq!(
        run.transition(RunTransition::ModelInvoked),
        Err(RunTransitionError::StepIncomplete)
    );
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    assert_eq!(
        run.transition(RunTransition::ModelInvoked),
        Ok(RunStatus::ApplyingResponse)
    );
}

#[test]
fn run_rejects_second_active_step_and_incomplete_step_completion() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();

    assert_eq!(
        run.begin_step(),
        Err(RunTransitionError::ActiveStepAlreadyExists)
    );
    assert_eq!(
        run.complete_step(&step_id),
        Err(RunTransitionError::StepIncomplete)
    );
}

#[test]
fn run_step_accepts_at_most_one_model_invocation() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    let invocation = ModelInvocation::new("request", "response");

    run.record_model_invocation(&step_id, invocation.clone())
        .unwrap();
    let error = run
        .record_model_invocation(&step_id, invocation)
        .unwrap_err();

    assert_eq!(error, RunTransitionError::InvocationAlreadyRecorded);
    assert_eq!(run.steps()[0].invocation().unwrap().response(), "response");
}

#[test]
fn tool_call_is_owned_by_a_run_step_and_advances_monotonically() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    let call = tool_call("provider-call-1");
    let call_id = call.id.clone();

    run.add_tool_call(&step_id, call).unwrap();
    run.advance_tool_call(&step_id, &call_id, ToolCallStatus::Ready)
        .unwrap();
    run.advance_tool_call(&step_id, &call_id, ToolCallStatus::Running)
        .unwrap();
    run.advance_tool_call(&step_id, &call_id, ToolCallStatus::Success)
        .unwrap();

    assert_eq!(run.steps()[0].tool_calls().len(), 1);
    assert_eq!(
        run.steps()[0].tool_calls()[0].status(),
        ToolCallStatus::Success
    );
    assert_eq!(
        run.advance_tool_call(&step_id, &call_id, ToolCallStatus::Running),
        Err(RunTransitionError::IllegalToolCallTransition {
            from: ToolCallStatus::Success,
            to: ToolCallStatus::Running,
        })
    );
}

#[test]
fn tool_call_cannot_be_added_to_another_or_inactive_step() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let active_step = run.begin_step().unwrap();
    let missing_step = RunStepId::new_v7();

    assert_eq!(
        run.add_tool_call(&missing_step, tool_call("missing")),
        Err(RunTransitionError::StepNotFound)
    );

    run.request_cancellation();
    assert_eq!(
        run.add_tool_call(&active_step, tool_call("cancelled")),
        Err(RunTransitionError::RunNotActive(RunStatus::Cancelling))
    );
}

#[test]
fn main_run_spec_uses_shared_interactive_unlimited_defaults() {
    let spec = RunSpec::main();

    assert_eq!(spec.kind, RunKind::Main);
    assert_eq!(spec.timeout, Duration::ZERO);
    assert_eq!(spec.input, InputMode::SessionQueue);
    assert_eq!(spec.interaction, InteractionMode::Interactive);
    assert_eq!(spec.events, EventRoute::Client);
    assert_eq!(spec.context, ResourceMode::Shared);
    assert_eq!(spec.workspace, ResourceMode::Shared);
    assert_eq!(spec.memory, MemoryMode::Enabled);
    assert_eq!(spec.tools, ToolScope::Full);
}

#[test]
fn sub_run_spec_is_isolated_noninteractive_and_parent_routed() {
    let spec = RunSpec::sub("reviewer", Duration::from_secs(60));

    assert_eq!(spec.kind, RunKind::Sub);
    assert_eq!(spec.name, "reviewer");
    assert_eq!(spec.timeout, Duration::from_secs(60));
    assert_eq!(spec.input, InputMode::Fixed);
    assert_eq!(spec.interaction, InteractionMode::NonInteractive);
    assert_eq!(spec.events, EventRoute::ParentRun);
    assert_eq!(spec.context, ResourceMode::Isolated);
    assert_eq!(spec.workspace, ResourceMode::Isolated);
    assert_eq!(spec.memory, MemoryMode::Disabled);
    assert_eq!(spec.tools, ToolScope::Restricted);
}

#[test]
fn derived_sub_spec_can_only_restrict_parent_capabilities() {
    let parent = RunSpec::main();
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();

    assert_eq!(sub.tools, ToolScope::Restricted);
    assert_eq!(sub.interaction, InteractionMode::NonInteractive);
    assert_eq!(sub.workspace, ResourceMode::Isolated);
    assert_eq!(sub.memory, MemoryMode::Disabled);
    assert_eq!(
        sub.clone().with_memory_mode(MemoryMode::Enabled),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

// ---------------------------------------------------------------------------
// #1272 Phase 2: per-turn drain-or-seal invariants
// ---------------------------------------------------------------------------

#[test]
fn created_only_transitions_into_draining_input() {
    let mut run = run();

    // Created 不得通过 Start 直接进入 PreparingContext (#1272)
    assert_eq!(
        run.transition(RunTransition::ContextPrepared),
        Err(RunTransitionError::IllegalTransition {
            from: RunStatus::Created,
            transition: RunTransition::ContextPrepared,
        })
    );
    assert_eq!(
        run.status(),
        RunStatus::Created,
        "被拒迁移不得污染 Run 状态"
    );

    assert_eq!(
        run.start_draining().map(|_| run.status()),
        Ok(RunStatus::DrainingInput)
    );
    assert!(run
        .events()
        .iter()
        .any(|event| matches!(event, RunDomainEvent::Started { .. })));
    assert!(run
        .events()
        .iter()
        .any(|event| matches!(event, RunDomainEvent::DrainingInput { .. })));
}

#[test]
fn finishing_transition_and_run_complete_bypass_are_not_in_the_machine() {
    for &from in ALL_RUN_STATUSES.iter() {
        for &transition in ALL_RUN_TRANSITIONS.iter() {
            let mut run = run_at_status(from);
            if from == RunStatus::InvokingModel && transition == RunTransition::ModelInvoked {
                let step_id = run.begin_step().unwrap();
                run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
                    .unwrap();
            }
            let next = try_transition(&mut run, transition);
            if next == Some(RunStatus::Completed) {
                assert_eq!(
                    (from, transition),
                    (RunStatus::DrainingInput, RunTransition::DrainEmptyAndSealed),
                    "Completed 仅可由 DrainingInput + DrainEmptyAndSealed 产生，\
                     实际来源为 {from:?} --{transition:?}",
                );
            }
        }
    }
}

#[test]
fn completed_only_via_draining_input_and_empty_and_sealed() {
    for &from in ALL_RUN_STATUSES.iter() {
        for &transition in ALL_RUN_TRANSITIONS.iter() {
            let mut run = run_at_status(from);
            if from == RunStatus::InvokingModel && transition == RunTransition::ModelInvoked {
                let step_id = run.begin_step().unwrap();
                run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
                    .unwrap();
            }
            let next = try_transition(&mut run, transition);
            if next == Some(RunStatus::Completed) {
                assert_eq!(
                    (from, transition),
                    (RunStatus::DrainingInput, RunTransition::DrainEmptyAndSealed),
                    "Completed 仅可由 DrainingInput + DrainEmptyAndSealed 产生，\
                     实际来源为 {from:?} --{transition:?}",
                );
            }
        }
    }
}

fn try_transition(run: &mut Run, transition: RunTransition) -> Option<RunStatus> {
    let outcome: Result<(), RunTransitionError> =
        if transition == RunTransition::DrainEmptyAndSealed {
            run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some(""))
        } else if transition == RunTransition::DrainInputs {
            run.apply_drain_decision(DrainDecision::Inputs, None)
        } else if transition == RunTransition::DrainInternalContinuation {
            run.apply_drain_decision(DrainDecision::InternalContinuation, None)
        } else if transition == RunTransition::StartDraining {
            run.start_draining()
        } else {
            run.transition(transition).map(|_| ())
        };
    match outcome {
        Ok(()) => Some(run.status()),
        Err(_) => None,
    }
}

#[test]
fn draining_input_drain_inputs_moves_to_preparing_context() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    assert_eq!(run.status(), RunStatus::PreparingContext);
}

#[test]
fn draining_input_internal_continuation_moves_to_preparing_context() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::InternalContinuation, None)
        .unwrap();
    assert_eq!(run.status(), RunStatus::PreparingContext);
}

#[test]
fn draining_input_empty_and_sealed_emits_completed_via_domain_only() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some("final"))
        .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(run.is_terminal());
    assert!(
        run.events().iter().any(|event| matches!(
            event,
            RunDomainEvent::Completed { result, .. } if result == "final"
        )),
        "domain should publish Completed on EmptyAndSealed, not the engine"
    );
}

#[test]
fn finalized_text_continue_stopblock_transition_each_enters_draining() {
    let transitions: &[RunTransition] = &[
        RunTransition::ContinueAfterResponse,
        RunTransition::ContinueAfterResponse,
        RunTransition::ContinueAfterResponse,
    ];
    for transition in transitions {
        let mut run = run_at_status(RunStatus::ApplyingResponse);
        let step_id = run
            .active_step_id()
            .expect("applying state has an active step");
        run.complete_step(&step_id).unwrap();

        run.transition(*transition).unwrap();
        assert_eq!(
            run.status(),
            RunStatus::DrainingInput,
            "text/continue/stopblock finalized → DrainingInput ({transition:?})"
        );
    }
}

#[test]
fn tools_completed_normal_path_enters_draining_not_preparing_context() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    run.transition(RunTransition::ToolsCompleted).unwrap();
    assert_eq!(run.status(), RunStatus::DrainingInput);
}

// ---------------------------------------------------------------------------
// #1272: set_pending_completion_result — explicit completion result API
// ---------------------------------------------------------------------------

#[test]
fn set_pending_completion_result_is_used_by_empty_and_sealed() {
    let mut run = run();
    run.start_draining().unwrap();
    run.set_pending_completion_result("explicit result".to_string());

    run.apply_drain_decision(DrainDecision::EmptyAndSealed, None)
        .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(run.is_terminal());
    assert!(
        run.events().iter().any(|event| matches!(
            event,
            RunDomainEvent::Completed { result, .. } if result == "explicit result"
        )),
        "Completed event must carry the result set via set_pending_completion_result"
    );
}

#[test]
fn empty_and_sealed_without_explicit_result_emits_empty_completed() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::EmptyAndSealed, None)
        .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(run.is_terminal());
    assert!(
        run.events().iter().any(|event| matches!(
            event,
            RunDomainEvent::Completed { result, .. } if result.is_empty()
        )),
        "Completed event must still be emitted with an empty result"
    );
}

#[test]
fn terminal_text_still_works_for_backward_compat() {
    let mut run = run();
    run.start_draining().unwrap();

    run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some("legacy text"))
        .unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(
        run.events().iter().any(|event| matches!(
            event,
            RunDomainEvent::Completed { result, .. } if result == "legacy text"
        )),
        "terminal_text parameter must still work for backward compat"
    );
}

#[test]
fn set_pending_completion_result_is_consumed_and_not_reused() {
    let mut run = run();
    run.start_draining().unwrap();
    run.set_pending_completion_result("first result".to_string());

    run.apply_drain_decision(DrainDecision::EmptyAndSealed, None)
        .unwrap();
    assert_eq!(run.status(), RunStatus::Completed);

    assert!(!run.events().iter().any(|event| matches!(
        event,
        RunDomainEvent::Completed { result, .. } if result.is_empty()
    )));
}

// ── #1272 drain epoch persistence ─────────────────────────────────
#[test]
fn new_run_starts_with_drain_epoch_zero() {
    let run = run();
    assert_eq!(run.next_drain_epoch(), 0);
}

#[test]
fn advance_drain_epoch_increments_monotonically() {
    let mut run = run();
    assert_eq!(run.next_drain_epoch(), 0);
    run.advance_drain_epoch();
    assert_eq!(run.next_drain_epoch(), 1);
    run.advance_drain_epoch();
    assert_eq!(run.next_drain_epoch(), 2);
}

#[test]
fn drain_epoch_persists_across_run_operations() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    run.transition(RunTransition::ContinueAfterResponse)
        .unwrap();
    run.complete_step(&step_id).unwrap();

    // Advance epoch to simulate drain persistence across run_loop re-entry.
    run.advance_drain_epoch();
    run.advance_drain_epoch();
    assert_eq!(run.next_drain_epoch(), 2);

    // Epoch survives terminal transition.
    run.apply_drain_decision(DrainDecision::EmptyAndSealed, Some("done"))
        .unwrap();
    assert_eq!(run.next_drain_epoch(), 2);
    assert_eq!(run.status(), RunStatus::Completed);
}

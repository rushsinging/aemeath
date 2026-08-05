use super::*;
use sdk::InteractionRequestId;
use std::time::Duration;

#[test]
fn run_domain_exposes_step_cancel_and_termination_only() {
    let state_source = include_str!("state.rs");
    let domain_source = include_str!("domain.rs");
    let event_source = include_str!("event.rs");

    for forbidden in [
        "    Cancelling,\n    Terminating,",
        "CancellationFinished",
        "RunCancellationRequest",
    ] {
        assert!(
            !state_source.contains(forbidden) && !domain_source.contains(forbidden),
            "Run Domain must not expose retired cancellation symbol: {forbidden}"
        );
    }

    for forbidden in [
        "request_cancellation",
        "finish_cancellation",
        "RuntimeLifecycleEvent::CancellationRequested",
        "RuntimeLifecycleEvent::Cancelled",
    ] {
        assert!(
            !domain_source.contains(forbidden) && !event_source.contains(forbidden),
            "Run Domain must not retain retired cancellation path: {forbidden}"
        );
    }
}

fn run() -> Run {
    Run::new(RunSpec::main(), None)
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
        RuntimeLifecycleEvent::AwaitingUser {
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
fn cancelling_interaction_restores_working_status_without_emitting_resumed() {
    let mut run = run_at_status(RunStatus::ExecutingTools);
    let request_id = InteractionRequestId::new_v7();
    let continuation = tool_continuation("call-cancel");
    run.begin_interaction(request_id.clone(), continuation.clone())
        .unwrap();
    run.drain_events();

    assert_eq!(run.cancel_interaction(&request_id).unwrap(), continuation);

    assert_eq!(run.status(), RunStatus::ExecutingTools);
    assert!(run.pending_interaction().is_none());
    assert!(!run
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Resumed { .. })));
    assert_eq!(
        run.cancel_interaction(&request_id),
        Err(RunTransitionError::NoPendingInteraction)
    );
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
            RunStatus::ExecutingTools,
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
        RuntimeLifecycleEvent::Terminated {
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
    run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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
        Some(RuntimeLifecycleEvent::Completed { result, .. }) if result == "final answer"
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
            RuntimeLifecycleEvent::Transitioned {
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
fn termination_and_completion_use_the_same_transition_event() {
    let mut terminated = run();
    terminated.start_draining().unwrap();
    terminated.request_termination(
        sdk::RunTerminationReason::SessionShutdown,
        sdk::ControlDeadline::from_unix_millis(0),
    );
    terminated.finish_termination().unwrap();

    assert!(terminated.events().iter().any(|event| matches!(
        event,
        RuntimeLifecycleEvent::Transitioned {
            from: RunStatus::DrainingInput,
            to: RunStatus::Terminating,
            reason: RunTransitionReason::TerminationRequested,
            ..
        }
    )));
    assert!(terminated.events().iter().any(|event| matches!(
        event,
        RuntimeLifecycleEvent::Transitioned {
            from: RunStatus::Terminating,
            to: RunStatus::Terminated,
            reason: RunTransitionReason::TerminationFinished,
            ..
        }
    )));
}

#[test]
fn transition_event_reports_runtime_owned_total_and_phase_elapsed() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();

    let transition = run
        .events()
        .iter()
        .rev()
        .find_map(|event| match event {
            RuntimeLifecycleEvent::Transitioned {
                to: RunStatus::PreparingContext,
                timing,
                ..
            } => Some(*timing),
            _ => None,
        })
        .expect("preparing-context transition timing");

    assert!(transition.total_elapsed_ms >= transition.phase_elapsed_ms);
    assert_eq!(transition.phase_elapsed_ms, 0);
}

#[test]
fn rejected_transition_does_not_emit_transitioned_event() {
    let mut run = run();

    let _ = run.transition(RunTransition::ModelInvoked);

    assert!(!run
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::Transitioned { .. })));
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
fn termination_is_two_phase_and_idempotent() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let reason = sdk::RunTerminationReason::SessionShutdown;
    let deadline = sdk::ControlDeadline::from_unix_millis(0);

    assert_eq!(
        run.request_termination(reason, deadline),
        RunTerminationRequest::Accepted
    );
    assert_eq!(run.status(), RunStatus::Terminating);
    assert_eq!(
        run.request_termination(reason, deadline),
        RunTerminationRequest::AlreadyTerminating
    );

    run.finish_termination().unwrap();

    assert_eq!(run.status(), RunStatus::Terminated);
    assert_eq!(
        run.request_termination(
            sdk::RunTerminationReason::SessionShutdown,
            sdk::ControlDeadline::from_unix_millis(0),
        ),
        RunTerminationRequest::AlreadyTerminal
    );
    let lifecycle: Vec<_> = run
        .events()
        .iter()
        .filter(|event| !matches!(event, RuntimeLifecycleEvent::Transitioned { .. }))
        .cloned()
        .collect();
    assert_eq!(
        lifecycle,
        vec![
            RuntimeLifecycleEvent::Started {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
            RuntimeLifecycleEvent::DrainingInput {
                run_id: run.id().clone(),
                parent_run_id: None,
            },
            RuntimeLifecycleEvent::TerminationRequested {
                run_id: run.id().clone(),
                parent_run_id: None,
                reason: sdk::RunTerminationReason::SessionShutdown,
                deadline,
            },
            RuntimeLifecycleEvent::Terminated {
                run_id: run.id().clone(),
                parent_run_id: None,
                reason: sdk::RunTerminationReason::SessionShutdown,
            },
        ]
    );
}

#[test]
fn terminating_run_rejects_new_work() {
    let mut run = run();
    run.start_draining().unwrap();
    run.request_termination(
        sdk::RunTerminationReason::SessionShutdown,
        sdk::ControlDeadline::from_unix_millis(0),
    );

    assert!(matches!(
        run.begin_step(),
        Err(RunTransitionError::RunNotActive(RunStatus::Terminating))
    ));
    assert!(matches!(
        run.transition(RunTransition::BeginCompaction),
        Err(RunTransitionError::IllegalTransition {
            from: RunStatus::Terminating,
            transition: RunTransition::BeginCompaction,
        })
    ));
}

#[test]
fn termination_closes_the_active_step_and_rejects_late_completion() {
    let mut run = run();
    run.start_draining().unwrap();
    run.apply_drain_decision(DrainDecision::Inputs, None)
        .unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();

    run.request_termination(
        sdk::RunTerminationReason::SessionShutdown,
        sdk::ControlDeadline::from_unix_millis(0),
    );
    run.finish_termination().unwrap();

    assert_eq!(
        run.steps()[0].status(),
        RunStepStatus::CancellationUnconfirmed
    );
    assert!(matches!(
        run.complete_step(&step_id),
        Err(RunTransitionError::RunNotActive(RunStatus::Terminated))
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
        RunSpec::sub("derived", Duration::from_secs(30)),
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

const ALL_RUN_STATUSES: [RunStatus; 15] = [
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
    RunStatus::Terminating,
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Terminated,
];

const ALL_RUN_TRANSITIONS: [RunTransition; 18] = [
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
];

fn invoke_to_applying(run: &mut Run) -> RunStepId {
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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
                run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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

#[test]
fn configured_stop_hook_block_limit_controls_retry_exhaustion() {
    let mut run = Run::new_with_stop_hook_block_limit(RunSpec::main(), None, 2);

    assert_eq!(
        run.record_stop_hook_block(),
        StopHookBlockResult::Blocked { count: 1 }
    );
    assert_eq!(
        run.record_stop_hook_block(),
        StopHookBlockResult::Blocked { count: 2 }
    );
    assert_eq!(
        run.record_stop_hook_block(),
        StopHookBlockResult::RetryExhausted { count: 3 }
    );
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
    run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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
    let invocation = ModelInvocation::new("response");

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
    run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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

    run.request_termination(
        sdk::RunTerminationReason::SessionShutdown,
        sdk::ControlDeadline::from_unix_millis(0),
    );
    assert_eq!(
        run.add_tool_call(&active_step, tool_call("terminated")),
        Err(RunTransitionError::RunNotActive(RunStatus::Terminating))
    );
}

#[test]
fn main_run_spec_uses_shared_interactive_unlimited_defaults() {
    let spec = RunSpec::main();

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
fn restricted_run_spec_is_isolated_noninteractive_and_parent_routed() {
    let spec = RunSpec::sub("reviewer", Duration::from_secs(60));

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
fn standalone_restricted_spec_applies_explicit_capability_policy() {
    let spec = RunSpec::sub("restricted", Duration::from_secs(30));

    assert_eq!(
        spec.clone().with_input(InputMode::SessionQueue),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.clone().with_interaction(InteractionMode::Interactive),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.clone().with_events(EventRoute::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.clone().with_context(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.clone().with_workspace(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.clone().with_memory_mode(MemoryMode::Enabled),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        spec.with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn standalone_full_spec_allows_capability_reconfiguration_without_parent_ceiling() {
    let spec = RunSpec::main()
        .with_input(InputMode::Fixed)
        .unwrap()
        .with_interaction(InteractionMode::NonInteractive)
        .unwrap()
        .with_events(EventRoute::ParentRun)
        .unwrap()
        .with_context(ResourceMode::Isolated)
        .unwrap()
        .with_workspace(ResourceMode::Isolated)
        .unwrap()
        .with_memory_mode(MemoryMode::Disabled)
        .unwrap()
        .with_tool_scope(ToolScope::Restricted)
        .unwrap();

    assert_eq!(spec.input, InputMode::Fixed);
    assert_eq!(spec.interaction, InteractionMode::NonInteractive);
    assert_eq!(spec.events, EventRoute::ParentRun);
    assert_eq!(spec.context, ResourceMode::Isolated);
    assert_eq!(spec.workspace, ResourceMode::Isolated);
    assert_eq!(spec.memory, MemoryMode::Disabled);
    assert_eq!(spec.tools, ToolScope::Restricted);
}

#[test]
fn derived_run_uses_parent_ceiling_for_relaxation_and_escalation() {
    let parent = RunSpec::main()
        .with_memory_mode(MemoryMode::Disabled)
        .unwrap()
        .with_interaction_kind(InteractionBindingMode::ParentMediated)
        .unwrap();
    let derived = parent
        .derive_sub("derived", Duration::from_secs(30))
        .unwrap();

    assert_eq!(
        derived.clone().with_memory_mode(MemoryMode::Enabled),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        derived
            .clone()
            .with_interaction_kind(InteractionBindingMode::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert!(derived
        .with_interaction_kind(InteractionBindingMode::Unavailable)
        .is_ok());
}

#[test]
fn validate_against_depends_only_on_effective_capabilities() {
    let parent = RunSpec::main()
        .with_memory_mode(MemoryMode::Disabled)
        .unwrap()
        .with_hooks(HookBindingMode::BoundaryOnly)
        .unwrap();
    let valid = RunSpec::sub("valid", Duration::from_secs(30))
        .with_hooks(HookBindingMode::BoundaryOnly)
        .unwrap();
    let elevated_memory = RunSpec::main()
        .with_timeout(Duration::from_secs(30))
        .unwrap()
        .with_input(InputMode::Fixed)
        .unwrap()
        .with_interaction(InteractionMode::NonInteractive)
        .unwrap()
        .with_events(EventRoute::ParentRun)
        .unwrap()
        .with_context(ResourceMode::Isolated)
        .unwrap()
        .with_workspace(ResourceMode::Isolated)
        .unwrap()
        .with_tool_scope(ToolScope::Restricted)
        .unwrap()
        .with_hooks(HookBindingMode::BoundaryOnly)
        .unwrap();

    assert_eq!(valid.validate_against(&parent), Ok(()));
    assert_eq!(
        elevated_memory.validate_against(&parent),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn derive_sub_from_main_can_relax_memory_only() {
    let parent = RunSpec::main();
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();

    // Defaults: most restrictive Sub profile
    assert_eq!(sub.tools, ToolScope::Restricted);
    assert_eq!(sub.interaction, InteractionMode::NonInteractive);
    assert_eq!(sub.workspace, ResourceMode::Isolated);
    assert_eq!(sub.memory, MemoryMode::Disabled);

    // Memory CAN relax to parent ceiling (main has Enabled).
    let sub = sub.with_memory_mode(MemoryMode::Enabled).unwrap();
    assert_eq!(sub.memory, MemoryMode::Enabled);

    // Fixed-profile fields CANNOT relax — even when parent allows.
    assert_eq!(
        sub.clone().with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_input(InputMode::SessionQueue),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_interaction(InteractionMode::Interactive),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_events(EventRoute::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_context(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.with_workspace(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn standalone_restricted_spec_rejects_fixed_profile_relaxation() {
    // Standalone sub (ceiling=None) must also respect the fixed sub profile.
    let sub = RunSpec::sub("r", Duration::from_secs(30));
    assert_eq!(
        sub.clone().with_input(InputMode::SessionQueue),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_interaction(InteractionMode::Interactive),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_events(EventRoute::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_context(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.clone().with_workspace(ResourceMode::Shared),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub.with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn standalone_restricted_spec_rejects_memory_enabled() {
    // Standalone sub has no ceiling → memory must stay Disabled.
    assert_eq!(
        RunSpec::sub("r", Duration::from_secs(30)).with_memory_mode(MemoryMode::Enabled),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn standalone_restricted_spec_allows_memory_disabled() {
    // Staying at the default (Disabled) is always allowed.
    assert!(RunSpec::sub("r", Duration::from_secs(30))
        .with_memory_mode(MemoryMode::Disabled)
        .is_ok());
}

#[test]
fn derived_run_can_relax_memory_when_parent_allows() {
    let parent = RunSpec::main(); // memory = Enabled
    let sub = parent.derive_sub("child", Duration::from_secs(10)).unwrap();
    // Memory can relax up to parent ceiling.
    let sub = sub.with_memory_mode(MemoryMode::Enabled).unwrap();
    assert_eq!(sub.memory, MemoryMode::Enabled);
}

// ── Task 3: table-driven capability contraction ───────────────────

#[test]
fn capability_input_monotonic_contraction() {
    let cases = [
        (InputMode::SessionQueue, InputMode::Fixed, true),
        // SessionQueue → SessionQueue REJECTED: sub fixed profile is Fixed
        (InputMode::SessionQueue, InputMode::SessionQueue, false),
        (InputMode::Fixed, InputMode::Fixed, true),
        (InputMode::Fixed, InputMode::SessionQueue, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_input(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_input(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "input parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_interaction_monotonic_contraction() {
    let cases = [
        (
            InteractionMode::Interactive,
            InteractionMode::NonInteractive,
            true,
        ),
        // Interactive → Interactive REJECTED: sub fixed profile is NonInteractive
        (
            InteractionMode::Interactive,
            InteractionMode::Interactive,
            false,
        ),
        (
            InteractionMode::NonInteractive,
            InteractionMode::NonInteractive,
            true,
        ),
        (
            InteractionMode::NonInteractive,
            InteractionMode::Interactive,
            false,
        ),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_interaction(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_interaction(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "interaction parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_events_monotonic_contraction() {
    let cases = [
        (EventRoute::Client, EventRoute::ParentRun, true),
        // Client → Client REJECTED: sub fixed profile is ParentRun
        (EventRoute::Client, EventRoute::Client, false),
        (EventRoute::ParentRun, EventRoute::ParentRun, true),
        (EventRoute::ParentRun, EventRoute::Client, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_events(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_events(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "events parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_context_monotonic_contraction() {
    let cases = [
        (ResourceMode::Shared, ResourceMode::Isolated, true),
        // Shared → Shared REJECTED: sub fixed profile is Isolated
        (ResourceMode::Shared, ResourceMode::Shared, false),
        (ResourceMode::Isolated, ResourceMode::Isolated, true),
        (ResourceMode::Isolated, ResourceMode::Shared, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_context(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_context(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "context parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_workspace_monotonic_contraction() {
    let cases = [
        (ResourceMode::Shared, ResourceMode::Isolated, true),
        // Shared → Shared REJECTED: sub fixed profile is Isolated
        (ResourceMode::Shared, ResourceMode::Shared, false),
        (ResourceMode::Isolated, ResourceMode::Isolated, true),
        (ResourceMode::Isolated, ResourceMode::Shared, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_workspace(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_workspace(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "workspace parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_memory_monotonic_contraction() {
    let cases = [
        (MemoryMode::Enabled, MemoryMode::Disabled, true),
        (MemoryMode::Enabled, MemoryMode::Enabled, true),
        (MemoryMode::Disabled, MemoryMode::Disabled, true),
        (MemoryMode::Disabled, MemoryMode::Enabled, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_memory_mode(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_memory_mode(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "memory parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn capability_tools_monotonic_contraction() {
    let cases = [
        (ToolScope::Full, ToolScope::Restricted, true),
        // Full → Full REJECTED: sub fixed profile is Restricted
        (ToolScope::Full, ToolScope::Full, false),
        (ToolScope::Restricted, ToolScope::Restricted, true),
        (ToolScope::Restricted, ToolScope::Full, false),
    ];
    for (parent_val, child_val, expect_ok) in cases {
        let parent = RunSpec::main().with_tool_scope(parent_val).unwrap();
        let sub = parent.derive_sub("test", Duration::from_secs(10)).unwrap();
        let result = sub.with_tool_scope(child_val);
        assert_eq!(
            result.is_ok(),
            expect_ok,
            "tools parent={parent_val:?} child={child_val:?} expect_ok={expect_ok}"
        );
    }
}

#[test]
fn nested_derived_run_cannot_restore_capabilities() {
    // main → sub1 (disable memory, restrict tools) → sub2
    let parent = RunSpec::main();
    let sub1 = parent
        .derive_sub("sub1", Duration::from_secs(60))
        .unwrap()
        .with_memory_mode(MemoryMode::Disabled)
        .unwrap()
        .with_tool_scope(ToolScope::Restricted)
        .unwrap();

    let sub2 = sub1.derive_sub("sub2", Duration::from_secs(30)).unwrap();

    // sub2 inherits sub1's effective caps, cannot re-enable memory or tools
    assert_eq!(
        sub2.clone().with_memory_mode(MemoryMode::Enabled),
        Err(RunSpecError::CapabilityEscalation)
    );
    assert_eq!(
        sub2.with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn nested_derived_run_can_further_restrict() {
    let parent = RunSpec::main();
    let sub1 = parent
        .derive_sub("sub1", Duration::from_secs(60))
        .unwrap()
        .with_memory_mode(MemoryMode::Disabled)
        .unwrap();

    // sub2 can stay at Disabled or go even more restrictive (already at floor)
    let sub2 = sub1.derive_sub("sub2", Duration::from_secs(30)).unwrap();
    assert_eq!(sub2.memory, MemoryMode::Disabled);

    let sub2 = sub2.with_memory_mode(MemoryMode::Disabled).unwrap();
    assert_eq!(sub2.memory, MemoryMode::Disabled);
}

#[test]
fn timeout_parent_zero_is_infinite() {
    let parent = RunSpec::main(); // timeout = 0
                                  // child can have any timeout
    assert!(parent.derive_sub("a", Duration::ZERO).is_ok());
    assert!(parent.derive_sub("b", Duration::from_secs(1)).is_ok());
    assert!(parent.derive_sub("c", Duration::from_secs(3600)).is_ok());
}

#[test]
fn timeout_child_must_not_exceed_finite_parent() {
    let parent = RunSpec::main()
        .with_timeout(Duration::from_secs(120))
        .unwrap();

    // child <= 120 → ok
    assert!(parent.derive_sub("ok1", Duration::from_secs(120)).is_ok());
    assert!(parent.derive_sub("ok2", Duration::from_secs(60)).is_ok());

    // child > 120 → error
    assert_eq!(
        parent.derive_sub("bad", Duration::from_secs(121)),
        Err(RunSpecError::CapabilityEscalation)
    );

    // child == 0 (infinite) while parent is finite → error
    assert_eq!(
        parent.derive_sub("infinite", Duration::ZERO),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn with_timeout_respects_parent_ceiling() {
    let parent = RunSpec::main()
        .with_timeout(Duration::from_secs(60))
        .unwrap();

    let sub = parent.derive_sub("sub", Duration::from_secs(30)).unwrap();

    // OK: child timeout ≤ parent
    assert!(sub.clone().with_timeout(Duration::from_secs(60)).is_ok());
    assert!(sub.clone().with_timeout(Duration::from_secs(30)).is_ok());

    // Error: child > parent
    assert_eq!(
        sub.clone().with_timeout(Duration::from_secs(61)),
        Err(RunSpecError::CapabilityEscalation)
    );

    // Error: child infinite while parent is finite
    assert_eq!(
        sub.with_timeout(Duration::ZERO),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn derive_sub_defaults_keep_restricted_profile() {
    let parent = RunSpec::main();
    let sub = parent.derive_sub("child", Duration::from_secs(10)).unwrap();

    assert_eq!(sub.input, InputMode::Fixed);
    assert_eq!(sub.interaction, InteractionMode::NonInteractive);
    assert_eq!(sub.events, EventRoute::ParentRun);
    assert_eq!(sub.context, ResourceMode::Isolated);
    assert_eq!(sub.workspace, ResourceMode::Isolated);
    assert_eq!(sub.memory, MemoryMode::Disabled);
    assert_eq!(sub.tools, ToolScope::Restricted);
    assert_eq!(sub.timeout, Duration::from_secs(10));
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
        .any(|event| matches!(event, RuntimeLifecycleEvent::Started { .. })));
    assert!(run
        .events()
        .iter()
        .any(|event| matches!(event, RuntimeLifecycleEvent::DrainingInput { .. })));
}

#[test]
fn finishing_transition_and_run_complete_bypass_are_not_in_the_machine() {
    for &from in ALL_RUN_STATUSES.iter() {
        for &transition in ALL_RUN_TRANSITIONS.iter() {
            let mut run = run_at_status(from);
            if from == RunStatus::InvokingModel && transition == RunTransition::ModelInvoked {
                let step_id = run.begin_step().unwrap();
                run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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
                run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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
            RuntimeLifecycleEvent::Completed { result, .. } if result == "final"
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
            RuntimeLifecycleEvent::Completed { result, .. } if result == "explicit result"
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
            RuntimeLifecycleEvent::Completed { result, .. } if result.is_empty()
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
            RuntimeLifecycleEvent::Completed { result, .. } if result == "legacy text"
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
        RuntimeLifecycleEvent::Completed { result, .. } if result.is_empty()
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
    run.record_model_invocation(&step_id, ModelInvocation::new("response"))
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

// ── #1248 Task 1: capability-semantic enums ──

#[test]
fn interaction_value_domain() {
    // Client is most permissive, Unavailable least.
    assert!(InteractionBindingMode::Unavailable.is_within(&InteractionBindingMode::ParentMediated));
    assert!(InteractionBindingMode::Unavailable.is_within(&InteractionBindingMode::Client));
    assert!(InteractionBindingMode::ParentMediated.is_within(&InteractionBindingMode::Client));
    // Escalation: permissive value exceeds restrictive ceiling.
    assert!(!InteractionBindingMode::ParentMediated.is_within(&InteractionBindingMode::Unavailable));
    assert!(!InteractionBindingMode::Client.is_within(&InteractionBindingMode::ParentMediated));
    assert!(!InteractionBindingMode::Client.is_within(&InteractionBindingMode::Unavailable));
}

#[test]
fn hook_value_domain() {
    assert!(HookBindingMode::BoundaryOnly.is_within(&HookBindingMode::Full));
    assert!(!HookBindingMode::Full.is_within(&HookBindingMode::BoundaryOnly));
}

#[test]
fn reasoning_value_domain() {
    // NoOp < Inherit < Fixed < Adaptive
    assert!(ReasoningBindingMode::NoOp.is_within(&ReasoningBindingMode::Inherit));
    assert!(ReasoningBindingMode::NoOp.is_within(&ReasoningBindingMode::Fixed));
    assert!(ReasoningBindingMode::NoOp.is_within(&ReasoningBindingMode::Adaptive));
    assert!(ReasoningBindingMode::Inherit.is_within(&ReasoningBindingMode::Fixed));
    assert!(ReasoningBindingMode::Inherit.is_within(&ReasoningBindingMode::Adaptive));
    assert!(ReasoningBindingMode::Fixed.is_within(&ReasoningBindingMode::Adaptive));

    // Escalation
    assert!(!ReasoningBindingMode::Adaptive.is_within(&ReasoningBindingMode::Fixed));
    assert!(!ReasoningBindingMode::Adaptive.is_within(&ReasoningBindingMode::Inherit));
    assert!(!ReasoningBindingMode::Fixed.is_within(&ReasoningBindingMode::Inherit));
    assert!(!ReasoningBindingMode::Fixed.is_within(&ReasoningBindingMode::NoOp));
    assert!(!ReasoningBindingMode::Inherit.is_within(&ReasoningBindingMode::NoOp));
}

#[test]
fn main_spec_has_client_interaction_full_hook_adaptive_reasoning() {
    let spec = RunSpec::main();
    assert_eq!(spec.interaction_binding(), InteractionBindingMode::Client);
    assert_eq!(spec.hook_binding(), HookBindingMode::Full);
    assert_eq!(spec.reasoning_binding(), ReasoningBindingMode::Adaptive);
}

#[test]
fn standalone_restricted_spec_has_parent_mediated_interaction_boundary_hook_inherit_reasoning() {
    let spec = RunSpec::sub("reviewer", Duration::from_secs(60));
    assert_eq!(
        spec.interaction_binding(),
        InteractionBindingMode::ParentMediated
    );
    assert_eq!(spec.hook_binding(), HookBindingMode::BoundaryOnly);
    assert_eq!(spec.reasoning_binding(), ReasoningBindingMode::Inherit);
}

#[test]
fn derive_sub_can_relax_interaction_to_parent_ceiling() {
    let parent = RunSpec::main(); // InteractionBindingMode::Client
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();
    assert_eq!(
        sub.interaction_binding(),
        InteractionBindingMode::ParentMediated
    );

    // Can relax to parent ceiling (Client).
    let sub = sub
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap();
    assert_eq!(sub.interaction_binding(), InteractionBindingMode::Client);
}

#[test]
fn derive_sub_rejects_interaction_escalation() {
    // Parent with ParentMediated ceiling — sub can't relax to Client.
    let parent = RunSpec::main()
        .with_interaction_kind(InteractionBindingMode::ParentMediated)
        .unwrap(); // ceiling = ParentMediated
    let sub = parent.derive_sub("sub", Duration::from_secs(30)).unwrap();
    assert_eq!(
        sub.interaction_binding(),
        InteractionBindingMode::ParentMediated
    );

    // Exceeds parent ceiling (ParentMediated) → escalation.
    assert_eq!(
        sub.with_interaction_kind(InteractionBindingMode::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn derive_sub_can_relax_hook_to_parent_ceiling() {
    let parent = RunSpec::main(); // HookBindingMode::Full
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();
    assert_eq!(sub.hook_binding(), HookBindingMode::BoundaryOnly);

    let sub = sub.with_hooks(HookBindingMode::Full).unwrap();
    assert_eq!(sub.hook_binding(), HookBindingMode::Full);
}

#[test]
fn derive_sub_rejects_hook_escalation() {
    let parent = RunSpec::main()
        .with_hooks(HookBindingMode::BoundaryOnly)
        .unwrap(); // ceiling = BoundaryOnly
    let sub = parent.derive_sub("sub", Duration::from_secs(30)).unwrap();
    assert_eq!(sub.hook_binding(), HookBindingMode::BoundaryOnly);

    assert_eq!(
        sub.with_hooks(HookBindingMode::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn derive_sub_can_relax_reasoning_to_parent_ceiling() {
    let parent = RunSpec::main(); // ReasoningBindingMode::Adaptive
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();
    assert_eq!(sub.reasoning_binding(), ReasoningBindingMode::Inherit);

    let sub = sub.with_reasoning(ReasoningBindingMode::Adaptive).unwrap();
    assert_eq!(sub.reasoning_binding(), ReasoningBindingMode::Adaptive);
}

#[test]
fn derive_sub_rejects_reasoning_escalation() {
    let parent = RunSpec::main()
        .with_reasoning(ReasoningBindingMode::Fixed)
        .unwrap(); // ceiling = Fixed
    let sub = parent.derive_sub("sub", Duration::from_secs(30)).unwrap();
    assert_eq!(sub.reasoning_binding(), ReasoningBindingMode::Inherit);

    assert_eq!(
        sub.with_reasoning(ReasoningBindingMode::Adaptive),
        Err(RunSpecError::CapabilityEscalation)
    );
}

#[test]
fn standalone_restricted_spec_can_relax_unpinned_interaction_capability() {
    let spec = RunSpec::sub("r", Duration::from_secs(30))
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap();
    assert_eq!(spec.interaction_binding(), InteractionBindingMode::Client);
}

#[test]
fn standalone_restricted_spec_can_set_interaction_unavailable() {
    let spec = RunSpec::sub("r", Duration::from_secs(30))
        .with_interaction_kind(InteractionBindingMode::Unavailable)
        .unwrap();
    assert_eq!(
        spec.interaction_binding(),
        InteractionBindingMode::Unavailable
    );
}

#[test]
fn nested_derived_run_can_further_restrict_interaction() {
    let parent = RunSpec::main(); // InteractionBindingMode::Client
    let sub1 = parent
        .derive_sub("sub1", Duration::from_secs(60))
        .unwrap()
        .with_interaction_kind(InteractionBindingMode::ParentMediated)
        .unwrap(); // ceiling = Client

    // sub2 inherits sub1's effective values as ceiling
    let sub2 = sub1.derive_sub("sub2", Duration::from_secs(30)).unwrap();
    assert_eq!(
        sub2.interaction_binding(),
        InteractionBindingMode::ParentMediated
    );

    // Can relax to ParentMediated (parent ceiling was Client, effective was ParentMediated)
    // sub1's effective interaction_kind = ParentMediated, so sub2's ceiling = ParentMediated
    let sub2 = sub2
        .with_interaction_kind(InteractionBindingMode::ParentMediated)
        .unwrap();
    assert_eq!(
        sub2.interaction_binding(),
        InteractionBindingMode::ParentMediated
    );

    // Cannot relax to Client (exceeds sub1's effective ceiling)
    assert_eq!(
        sub2.with_interaction_kind(InteractionBindingMode::Client),
        Err(RunSpecError::CapabilityEscalation)
    );
}

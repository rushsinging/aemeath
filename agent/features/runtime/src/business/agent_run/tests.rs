use super::*;
use std::time::Duration;

fn run() -> Run {
    Run::new(RunSpec::new("main", Duration::ZERO), None)
}

#[test]
fn run_follows_the_happy_path_to_completed() {
    let mut run = run();

    run.transition(RunTransition::Start).unwrap();
    run.transition(RunTransition::ContextPrepared).unwrap();
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    run.transition(RunTransition::ResponseWithoutTools).unwrap();
    run.complete_step(&step_id).unwrap();
    run.complete("final answer").unwrap();

    assert_eq!(run.status(), RunStatus::Completed);
    assert!(run.is_terminal());
    assert!(matches!(
        run.events().last(),
        Some(RunDomainEvent::Completed { result, .. }) if result == "final answer"
    ));
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
    run.transition(RunTransition::Start).unwrap();
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
    assert_eq!(
        run.events(),
        &[
            RunDomainEvent::Started {
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
    run.transition(RunTransition::Start).unwrap();
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
    run.transition(RunTransition::Start).unwrap();
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
    run.transition(RunTransition::Start).unwrap();
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

    run.transition(RunTransition::Start).unwrap();
    run.fail("failed").unwrap();

    assert_eq!(run.parent_id(), Some(&parent));
    assert!(run
        .events()
        .iter()
        .all(|event| event.parent_run_id() == Some(&parent)));
}

const ALL_RUN_STATUSES: [RunStatus; 13] = [
    RunStatus::Created,
    RunStatus::PreparingContext,
    RunStatus::InvokingModel,
    RunStatus::ApplyingResponse,
    RunStatus::AwaitingToolApproval,
    RunStatus::ExecutingTools,
    RunStatus::AwaitingUser,
    RunStatus::Compacting,
    RunStatus::Finishing,
    RunStatus::Cancelling,
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Cancelled,
];

const ALL_RUN_TRANSITIONS: [RunTransition; 16] = [
    RunTransition::Start,
    RunTransition::BeginCompaction,
    RunTransition::CompactionCompleted,
    RunTransition::ContextPrepared,
    RunTransition::RetryModel,
    RunTransition::ModelContextExceeded,
    RunTransition::ModelInvoked,
    RunTransition::ResponseWithTools,
    RunTransition::ResponseWithoutTools,
    RunTransition::ContinueAfterResponse,
    RunTransition::ToolsApproved,
    RunTransition::AwaitUser,
    RunTransition::UserResumed,
    RunTransition::ToolsCompleted,
    RunTransition::Finish,
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

    run.transition(RunTransition::Start).unwrap();
    match status {
        RunStatus::Created => unreachable!(),
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
        RunStatus::Finishing => {
            invoke_to_applying(&mut run);
            run.transition(RunTransition::ResponseWithoutTools).unwrap();
        }
        RunStatus::Cancelling => {
            assert_eq!(run.request_cancellation(), RunCancellationRequest::Accepted);
        }
        RunStatus::Completed => {
            let step_id = invoke_to_applying(&mut run);
            run.transition(RunTransition::ResponseWithoutTools).unwrap();
            run.complete_step(&step_id).unwrap();
            run.transition(RunTransition::Finish).unwrap();
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
        (RunStatus::Created, RunTransition::Start) => Some(RunStatus::PreparingContext),
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
        (RunStatus::ApplyingResponse, RunTransition::ResponseWithoutTools) => {
            Some(RunStatus::Finishing)
        }
        (RunStatus::ApplyingResponse, RunTransition::ContinueAfterResponse) => {
            Some(RunStatus::PreparingContext)
        }
        (RunStatus::AwaitingToolApproval, RunTransition::ToolsApproved) => {
            Some(RunStatus::ExecutingTools)
        }
        (RunStatus::AwaitingToolApproval, RunTransition::AwaitUser)
        | (RunStatus::ExecutingTools, RunTransition::AwaitUser) => Some(RunStatus::AwaitingUser),
        (RunStatus::AwaitingUser, RunTransition::UserResumed)
        | (RunStatus::ExecutingTools, RunTransition::ToolsCompleted) => {
            Some(RunStatus::PreparingContext)
        }
        (RunStatus::Finishing, RunTransition::Finish) => Some(RunStatus::Completed),
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

fn tool_call(provider_id: &str) -> crate::business::agent::ToolCall {
    crate::business::agent::ToolCall {
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
fn run_cannot_complete_while_step_is_active() {
    let mut run = run_at_status(RunStatus::InvokingModel);
    let step_id = run.begin_step().unwrap();
    run.record_model_invocation(&step_id, ModelInvocation::new("request", "response"))
        .unwrap();
    run.transition(RunTransition::ModelInvoked).unwrap();
    run.transition(RunTransition::ResponseWithoutTools).unwrap();

    assert_eq!(
        run.complete("invalid"),
        Err(RunTransitionError::StepIncomplete)
    );
    assert_eq!(run.status(), RunStatus::Finishing);
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
    assert_eq!(spec.tools, ToolScope::Restricted);
}

#[test]
fn derived_sub_spec_can_only_restrict_parent_capabilities() {
    let parent = RunSpec::main();
    let sub = parent.derive_sub("coder", Duration::from_secs(30)).unwrap();

    assert_eq!(sub.tools, ToolScope::Restricted);
    assert_eq!(sub.interaction, InteractionMode::NonInteractive);
    assert_eq!(sub.workspace, ResourceMode::Isolated);
    assert_eq!(
        sub.with_tool_scope(ToolScope::Full),
        Err(RunSpecError::CapabilityEscalation)
    );
}

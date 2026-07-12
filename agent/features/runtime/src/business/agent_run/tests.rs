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

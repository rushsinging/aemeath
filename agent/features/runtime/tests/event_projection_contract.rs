use runtime::adapters::event_projection::project_domain_event;
use runtime::domain::agent_run::RunDomainEvent;
use sdk::{ChatEvent, ControlDeadline, RunId, RunStepId, RunTerminationReason};

#[test]
fn step_identity_is_preserved_by_domain_projection() {
    let run_id = RunId::new_v7();
    let parent_run_id = RunId::new_v7();
    let step_id = RunStepId::new_v7();

    let projected = project_domain_event(RunDomainEvent::StepStarted {
        run_id: run_id.clone(),
        parent_run_id: Some(parent_run_id.clone()),
        step_id: step_id.clone(),
    });

    assert!(matches!(
        projected,
        ChatEvent::RunStepStarted {
            run_id: actual_run,
            parent_run_id: Some(actual_parent),
            step_id: actual_step,
        } if actual_run == run_id && actual_parent == parent_run_id && actual_step == step_id
    ));
}

#[test]
fn run_termination_contract_is_preserved_by_domain_projection() {
    let run_id = RunId::new_v7();
    let deadline = ControlDeadline::from_unix_millis(42);

    let projected = project_domain_event(RunDomainEvent::TerminationRequested {
        run_id: run_id.clone(),
        parent_run_id: None,
        reason: RunTerminationReason::UserExit,
        deadline,
    });

    assert!(matches!(
        projected,
        ChatEvent::RunTerminationRequested {
            run_id: actual_run,
            parent_run_id: None,
            reason: RunTerminationReason::UserExit,
            deadline: actual_deadline,
        } if actual_run == run_id && actual_deadline == deadline
    ));
}

#[test]
fn terminal_payload_is_preserved_by_domain_projection() {
    let run_id = RunId::new_v7();
    let projected = project_domain_event(RunDomainEvent::Failed {
        run_id: run_id.clone(),
        parent_run_id: None,
        error: "provider failed".to_string(),
    });

    assert!(matches!(
        projected,
        ChatEvent::RunFailed { run_id: actual_run, error, .. }
            if actual_run == run_id && error == "provider failed"
    ));
}

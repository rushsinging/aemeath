use super::*;
use crate::domain::agent_run::{ActiveRunPort, RunControl};

#[test]
fn cancel_step_only_cancels_current_step_scope() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let step_id = sdk::RunStepId::new_v7();
    let root = CancellationToken::new();
    let step = root.child_token();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    registry.activate_main(run_id.clone(), root.clone());
    registry.set_main_active_step(&run_id, step_id.clone(), step.clone());

    assert_eq!(
        registry.cancel_step(&run_id, Some(&step_id), deadline),
        sdk::CancelRunStepOutcome::Accepted
    );
    assert!(step.is_cancelled());
    assert!(!root.is_cancelled());
    assert_eq!(
        registry.take_control(&run_id),
        Some(RunControl::CancelStep { step_id, deadline })
    );
}

#[test]
fn terminate_preempts_cancel_step_and_cancels_root_scope() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let step_id = sdk::RunStepId::new_v7();
    let root = CancellationToken::new();
    let step = root.child_token();
    let cancel_deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);
    let terminate_deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_456);

    registry.activate_main(run_id.clone(), root.clone());
    registry.set_main_active_step(&run_id, step_id.clone(), step.clone());
    assert_eq!(
        registry.cancel_step(&run_id, Some(&step_id), cancel_deadline),
        sdk::CancelRunStepOutcome::Accepted
    );
    assert_eq!(
        registry.terminate(
            &run_id,
            sdk::RunTerminationReason::UserExit,
            terminate_deadline,
        ),
        sdk::TerminateRunOutcome::Accepted
    );
    assert!(root.is_cancelled());
    assert!(step.is_cancelled());
    assert_eq!(
        registry.take_control(&run_id),
        Some(RunControl::Terminate {
            reason: sdk::RunTerminationReason::UserExit,
            deadline: terminate_deadline,
        })
    );
}

#[test]
fn repeated_main_control_commands_are_idempotent() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let step_id = sdk::RunStepId::new_v7();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    registry.activate_main(run_id.clone(), CancellationToken::new());
    registry.set_main_active_step(&run_id, step_id.clone(), CancellationToken::new());
    assert_eq!(
        registry.cancel_step(&run_id, Some(&step_id), deadline),
        sdk::CancelRunStepOutcome::Accepted
    );
    assert_eq!(
        registry.cancel_step(&run_id, Some(&step_id), deadline),
        sdk::CancelRunStepOutcome::AlreadyCancelling
    );
    assert_eq!(
        registry.terminate(
            &run_id,
            sdk::RunTerminationReason::SessionShutdown,
            deadline,
        ),
        sdk::TerminateRunOutcome::Accepted
    );
    assert_eq!(
        registry.terminate(
            &run_id,
            sdk::RunTerminationReason::SessionShutdown,
            deadline,
        ),
        sdk::TerminateRunOutcome::AlreadyTerminating
    );
}

#[test]
fn registry_tracks_runs_independently() {
    let registry = ActiveRunRegistry::default();
    let parent = sdk::RunId::new_v7();
    let sub_a = sdk::RunId::new_v7();
    let sub_b = sdk::RunId::new_v7();
    let parent_token = CancellationToken::new();
    let sub_a_token = parent_token.child_token();
    let sub_b_token = parent_token.child_token();

    registry.activate(parent.clone(), parent_token.clone());
    registry.activate(sub_a.clone(), sub_a_token.clone());
    registry.activate(sub_b.clone(), sub_b_token.clone());

    assert_eq!(registry.active_ids().len(), 3);
    assert_eq!(registry.cancel(&sub_a), sdk::CancelRunOutcome::Accepted);
    assert!(sub_a_token.is_cancelled());
    assert!(!parent_token.is_cancelled());
    assert!(!sub_b_token.is_cancelled());

    registry.clear(&sub_a);
    assert_eq!(registry.active_ids().len(), 2);
    assert_eq!(registry.cancel(&parent), sdk::CancelRunOutcome::Accepted);
    assert!(parent_token.is_cancelled());
    assert!(sub_b_token.is_cancelled());
}

#[test]
fn cancel_is_synchronous_and_id_scoped() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let other = sdk::RunId::new_v7();
    let token = CancellationToken::new();
    registry.activate(run_id.clone(), token.clone());

    assert_eq!(registry.cancel(&other), sdk::CancelRunOutcome::NotFound);
    assert!(!token.is_cancelled());
    assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::Accepted);
    assert!(
        token.is_cancelled(),
        "token must be cancelled before return"
    );
    assert_eq!(
        registry.cancel(&run_id),
        sdk::CancelRunOutcome::AlreadyCancelling
    );
}

#[test]
fn terminal_claim_is_visible_to_late_cancel_until_clear() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    registry.activate(run_id.clone(), CancellationToken::new());

    assert!(registry.claim_terminal(&run_id));
    assert_eq!(
        registry.cancel(&run_id),
        sdk::CancelRunOutcome::AlreadyTerminal
    );
    registry.clear(&run_id);
    assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::NotFound);
}

#[test]
fn terminal_claim_blocks_late_cancellation_claim() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    registry.activate(run_id.clone(), CancellationToken::new());

    assert!(registry.claim_terminal(&run_id));
    assert!(!registry.claim_cancellation(&run_id));
}

#[test]
fn cancellation_wins_over_terminal_claim() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    registry.activate(run_id.clone(), CancellationToken::new());

    assert_eq!(registry.cancel(&run_id), sdk::CancelRunOutcome::Accepted);
    assert!(!registry.claim_terminal(&run_id));
}

#[test]
fn clear_only_removes_matching_run() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let other = sdk::RunId::new_v7();
    registry.activate(run_id.clone(), CancellationToken::new());

    registry.clear(&other);
    assert_eq!(registry.active_ids(), vec![run_id.clone()]);
    registry.clear(&run_id);
    assert!(registry.active_ids().is_empty());
}

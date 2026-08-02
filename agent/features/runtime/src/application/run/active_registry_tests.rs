use super::*;
use crate::domain::agent_run::{ActiveRunPort, RunControl};

#[test]
fn registry_does_not_duplicate_run_lifecycle_or_expose_legacy_cancel() {
    let registry_source = include_str!("active_registry.rs");
    let active_run_port_source = include_str!("../../domain/agent_run.rs");

    for retired in [
        "pub cancelling: bool",
        "pub terminal: bool",
        "fn claim_terminal(",
        "fn claim_cancellation(",
        "pub fn cancel(&self",
        "sdk::CancelRunOutcome",
    ] {
        assert!(
            !registry_source.contains(retired),
            "ActiveRunRegistry retains a second Run lifecycle source: {retired}"
        );
    }

    for retired in ["fn claim_terminal(", "fn claim_cancellation("] {
        assert!(
            !active_run_port_source.contains(retired),
            "ActiveRunPort retains lifecycle arbitration: {retired}"
        );
    }
}

#[test]
fn cancel_current_main_without_active_step_is_explicit_and_preserves_root_scope() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let root = CancellationToken::new();

    registry.activate_main(run_id, root.clone());

    assert_eq!(
        registry.cancel_current_main(sdk::ControlDeadline::from_unix_millis(1)),
        sdk::CancelCurrentRunOutcome::NoActiveStep
    );
    assert!(!root.is_cancelled());
}

#[test]
fn cancel_current_main_step_does_not_require_run_identity() {
    let registry = ActiveRunRegistry::default();
    let run_id = sdk::RunId::new_v7();
    let step_id = sdk::RunStepId::new_v7();
    let root = CancellationToken::new();
    let step = root.child_token();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    registry.activate_main(run_id.clone(), root.clone());
    registry.set_main_active_step(&run_id, step_id.clone(), step.clone());

    assert_eq!(
        registry.cancel_current_main(deadline),
        sdk::CancelCurrentRunOutcome::Accepted
    );
    assert!(step.is_cancelled());
    assert!(!root.is_cancelled());
    assert_eq!(
        registry.take_control(&run_id),
        Some(RunControl::CancelStep { step_id, deadline })
    );
}

#[test]
fn sub_run_does_not_replace_current_main_run() {
    let registry = ActiveRunRegistry::default();
    let main_id = sdk::RunId::new_v7();
    let sub_id = sdk::RunId::new_v7();
    let main_step_id = sdk::RunStepId::new_v7();
    let main_root = CancellationToken::new();
    let main_step = main_root.child_token();
    let sub_root = CancellationToken::new();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    registry.activate_main(main_id.clone(), main_root.clone());
    registry.set_main_active_step(&main_id, main_step_id, main_step.clone());
    registry.activate(sub_id, sub_root.clone());

    assert_eq!(
        registry.cancel_current_main(deadline),
        sdk::CancelCurrentRunOutcome::Accepted
    );
    assert!(main_step.is_cancelled());
    assert!(!main_root.is_cancelled());
    assert!(!sub_root.is_cancelled());
}

#[test]
fn clearing_old_main_does_not_clear_new_current_main() {
    let registry = ActiveRunRegistry::default();
    let old_id = sdk::RunId::new_v7();
    let new_id = sdk::RunId::new_v7();
    let new_step_id = sdk::RunStepId::new_v7();
    let new_root = CancellationToken::new();
    let new_step = new_root.child_token();
    let deadline = sdk::ControlDeadline::from_unix_millis(1_725_000_000_123);

    registry.activate_main(old_id.clone(), CancellationToken::new());
    registry.activate_main(new_id.clone(), new_root);
    registry.set_main_active_step(&new_id, new_step_id, new_step.clone());
    registry.clear(&old_id);

    assert_eq!(
        registry.cancel_current_main(deadline),
        sdk::CancelCurrentRunOutcome::Accepted
    );
    assert!(new_step.is_cancelled());
}

#[test]
fn cancel_current_main_without_active_run_is_explicit() {
    let registry = ActiveRunRegistry::default();
    assert_eq!(
        registry.cancel_current_main(sdk::ControlDeadline::from_unix_millis(1)),
        sdk::CancelCurrentRunOutcome::NoActiveRun
    );
}

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
    registry.clear(&sub_a);
    assert_eq!(registry.active_ids().len(), 2);
    assert!(!parent_token.is_cancelled());
    assert!(!sub_a_token.is_cancelled());
    assert!(!sub_b_token.is_cancelled());
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

use std::sync::{Arc, Mutex};

use super::{await_pending_tasks, StreamingInvocation, StreamingToolState};
use crate::application::loop_engine::chat::tools::ToolRoundResult;
use tokio_util::sync::CancellationToken;

#[test]
fn reset_for_invocation_replaces_step_cancellation_and_generation() {
    let first_cancel = CancellationToken::new();
    let second_cancel = CancellationToken::new();
    let first_step = sdk::RunStepId::new_v7();
    let second_step = sdk::RunStepId::new_v7();
    let mut state = StreamingToolState::default();

    let first = state.begin_invocation(&first_step, first_cancel.clone());
    let second = state.begin_invocation(&second_step, second_cancel.clone());

    assert_eq!(first.step_id, first_step);
    assert_eq!(first.generation, 1);
    assert_eq!(second.step_id, second_step.clone());
    assert_eq!(second.generation, 2);
    first.cancel.cancel();
    assert!(first.cancel.is_cancelled());
    assert!(!second.cancel.is_cancelled());
    assert!(state.accepts(&second));
    assert!(state.accepts(&StreamingInvocation::new(second_step, 2, second_cancel)));
    assert!(!state.accepts(&StreamingInvocation::new(
        sdk::RunStepId::new_v7(),
        1,
        first_cancel,
    )));
}

#[test]
fn reset_detaches_old_invocation_before_waiting_for_tasks() {
    let step_id = sdk::RunStepId::new_v7();
    let mut state = StreamingToolState::default();
    let invocation = state.begin_invocation(&step_id, CancellationToken::new());

    let handles = state.detach_invocation();

    assert!(handles.is_empty());
    assert!(invocation.cancel.is_cancelled());
    assert!(!state.accepts(&invocation));
    assert!(state.invocation.is_none());
}

#[tokio::test]
async fn reset_waits_for_cancelled_tasks_to_run_cleanup() {
    let cancellation = CancellationToken::new();
    let cleanup_order = Arc::new(Mutex::new(Vec::new()));
    let task_cancel = cancellation.clone();
    let task_order = cleanup_order.clone();
    let handle = tokio::spawn(async move {
        task_cancel.cancelled().await;
        task_order.lock().expect("cleanup order").push("cleanup");
    });

    cancellation.cancel();
    await_pending_tasks(vec![handle]).await;
    cleanup_order.lock().expect("cleanup order").push("reset");

    assert_eq!(
        cleanup_order.lock().expect("cleanup order").as_slice(),
        ["cleanup", "reset"]
    );
}

#[test]
fn completed_round_is_accepted_only_by_the_matching_invocation() {
    let first_step = sdk::RunStepId::new_v7();
    let second_step = sdk::RunStepId::new_v7();
    let mut state = StreamingToolState::default();
    let first = state.begin_invocation(&first_step, CancellationToken::new());
    let second = state.begin_invocation(&second_step, CancellationToken::new());

    let empty_round = || ToolRoundResult {
        results: Vec::new(),
        fuse_bypassed: Vec::new(),
        suspensions: Vec::new(),
        approvals: Vec::new(),
    };

    assert!(!state.accept_result(&first, empty_round()));
    assert!(state.results.is_empty());
    assert!(state.accept_result(&second, empty_round()));
    assert_eq!(state.results.len(), 1);
}

#[test]
fn cancellation_scope_is_not_the_run_root_scope() {
    let run_root = CancellationToken::new();
    let step_cancel = run_root.child_token();
    let step_id = sdk::RunStepId::new_v7();
    let mut state = StreamingToolState::default();
    let invocation = state.begin_invocation(&step_id, step_cancel.clone());

    step_cancel.cancel();

    assert!(invocation.cancel.is_cancelled());
    assert!(!run_root.is_cancelled());
}

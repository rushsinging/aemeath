use super::intent::ObserveRunStatus;
use super::interaction::UiRunId;
use super::model::ConversationModel;
use crate::tui::adapter::tui_runtime_event::{TuiRunStatus, TuiRunTiming};

fn timing(total_elapsed_ms: u64, phase_elapsed_ms: u64) -> TuiRunTiming {
    TuiRunTiming {
        total_elapsed_ms,
        phase_elapsed_ms,
    }
}

fn observe(
    model: &mut ConversationModel,
    run_id: &str,
    parent_run_id: Option<&str>,
    status: TuiRunStatus,
) -> Vec<super::change::ConversationChange> {
    model.apply(ObserveRunStatus {
        run_id: UiRunId::from(run_id),
        parent_run_id: parent_run_id.map(UiRunId::from),
        status,
        timing: timing(12_345, 678),
    })
}

#[test]
fn unknown_transition_creates_snapshot_and_main_identity() {
    let mut model = ConversationModel::default();

    let changes = observe(&mut model, "main-1", None, TuiRunStatus::PreparingContext);

    assert_eq!(model.run_state_snapshots().len(), 1);
    assert_eq!(model.active_main_run_id(), Some(&UiRunId::from("main-1")));
    let snapshot = model.active_main_run_snapshot().expect("main snapshot");
    assert_eq!(snapshot.total_elapsed_ms, 12_345);
    assert_eq!(snapshot.phase_elapsed_ms, 678);
    assert_eq!(changes.len(), 1);
}

#[test]
fn duplicate_transition_is_idempotent() {
    let mut model = ConversationModel::default();
    observe(&mut model, "main-1", None, TuiRunStatus::InvokingModel);

    let changes = observe(&mut model, "main-1", None, TuiRunStatus::InvokingModel);

    assert!(changes.is_empty());
    assert_eq!(model.run_state_snapshots().len(), 1);
}

#[test]
fn sub_run_does_not_replace_active_main_identity() {
    let mut model = ConversationModel::default();
    observe(&mut model, "main-1", None, TuiRunStatus::InvokingModel);

    observe(
        &mut model,
        "sub-1",
        Some("main-1"),
        TuiRunStatus::ExecutingTools,
    );

    assert_eq!(model.active_main_run_id(), Some(&UiRunId::from("main-1")));
    assert_eq!(model.run_state_snapshots().len(), 2);
}

#[test]
fn terminal_run_rejects_late_live_status() {
    let mut model = ConversationModel::default();
    observe(&mut model, "main-1", None, TuiRunStatus::Completed);

    let changes = observe(&mut model, "main-1", None, TuiRunStatus::InvokingModel);

    assert!(changes.is_empty());
    assert_eq!(
        model
            .active_main_run_snapshot()
            .map(|snapshot| snapshot.status),
        Some(TuiRunStatus::Completed)
    );
}

#[test]
fn new_main_replaces_terminal_main_identity() {
    let mut model = ConversationModel::default();
    observe(&mut model, "main-1", None, TuiRunStatus::Completed);

    observe(&mut model, "main-2", None, TuiRunStatus::Created);

    assert_eq!(model.active_main_run_id(), Some(&UiRunId::from("main-2")));
}

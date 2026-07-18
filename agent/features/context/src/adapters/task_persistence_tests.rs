use super::task_persistence::{TaskRestoreAdapter, TaskSnapshotSource};
use crate::domain::session::SnapshotState;
use task::{
    wire_task, BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskSnapshot,
    TaskSnapshotValidationError,
};

use std::sync::Arc;

fn batch_spec(name: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(name.to_owned()).expect("valid batch")
}

fn task_spec(name: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(name.to_owned(), String::new(), None, TaskPriority::Normal)
        .expect("valid task")
}

fn add_task(access: &dyn TaskAccess, name: &str) {
    access
        .create_batch(batch_spec(&format!("{name}-batch")), 1)
        .expect("create batch");
    access.create_task(task_spec(name), 2).expect("create task");
}

fn invalid_self_dependency_snapshot() -> TaskSnapshot {
    TaskSnapshot::decode(
        br#"{"schema_version":2,"revision":"1","tasks":[{"id":"1","batch":"1","subject":"invalid","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":["1"],"status":"pending","priority":"normal","created_at":1,"updated_at":1,"started_at":null,"completed_at":null}],"next_task_id":"2","next_batch_id":"2","current_batch":"1","batches":[{"id":"1","summary":"batch","status":"active","created_at":1,"last_active_turn":0,"silence_turns":0}]}"#,
    )
    .expect("fixture is valid snapshot wire data")
}

#[test]
fn legacy_writer_captures_tool_created_state_from_the_shared_backing() {
    let wiring = wire_task();
    let access = wiring.access();
    add_task(access.as_ref(), "created by tool");
    let source = TaskSnapshotSource::new(wiring.persist());
    let mut session = crate::domain::session::Session::new("s".into(), "/tmp".into());

    source
        .capture_legacy_session(&mut session)
        .expect("capture legacy session");

    let snapshot = session.tasks.expect("non-empty snapshot is written");
    assert_eq!(snapshot.tasks.len(), 1);
    assert_eq!(snapshot.tasks[0].subject, "created by tool");
    assert_eq!(snapshot.batches.len(), 1);
}

#[test]
fn legacy_writer_records_empty_state_instead_of_none() {
    let wiring = wire_task();
    let source = TaskSnapshotSource::new(wiring.persist());
    let mut session = crate::domain::session::Session::new("s".into(), "/tmp".into());

    source
        .capture_legacy_session(&mut session)
        .expect("capture empty state");

    let snapshot = session.tasks.expect("captured empty is not missing");
    assert!(snapshot.tasks.is_empty());
    assert!(snapshot.batches.is_empty());
}

#[test]
fn snapshot_source_always_returns_captured_for_nonempty_and_empty_live_state() {
    let empty_wiring = wire_task();
    let empty_source = TaskSnapshotSource::new(empty_wiring.persist());

    match empty_source.source() {
        SnapshotState::Captured(snapshot) => assert_eq!(snapshot, TaskSnapshot::empty()),
        state => panic!("empty live state must still be Captured, got {state:?}"),
    }

    let nonempty_wiring = wire_task();
    let access = nonempty_wiring.access();
    add_task(access.as_ref(), "persist me");
    let expected = nonempty_wiring.persist().collect_snapshot();
    let source = TaskSnapshotSource::new(nonempty_wiring.persist());

    assert_eq!(source.source(), SnapshotState::Captured(expected));
}

#[test]
fn restore_captured_snapshot_prepares_without_mutation_then_commit_replaces_live_state() {
    let source_wiring = wire_task();
    add_task(source_wiring.access().as_ref(), "restored");
    let captured = SnapshotState::Captured(source_wiring.persist().collect_snapshot());

    let target_wiring = wire_task();
    let target_access = target_wiring.access();
    let target_persist = target_wiring.persist();
    add_task(target_access.as_ref(), "stale");
    let before_prepare = target_persist.collect_snapshot();
    let adapter = TaskRestoreAdapter::new(Arc::clone(&target_persist));

    let prepared = adapter
        .prepare(&captured)
        .expect("captured valid snapshot must prepare");
    assert_eq!(target_persist.collect_snapshot(), before_prepare);

    adapter.commit(prepared);
    assert_eq!(
        target_persist.collect_snapshot(),
        source_wiring.persist().collect_snapshot()
    );
}

#[test]
fn restore_rejects_invalid_captured_snapshot_during_prepare_and_keeps_live_state() {
    let wiring = wire_task();
    let access = wiring.access();
    let persist = wiring.persist();
    add_task(access.as_ref(), "live");
    let before = persist.collect_snapshot();
    let adapter = TaskRestoreAdapter::new(Arc::clone(&persist));

    let result = adapter.prepare(&SnapshotState::Captured(invalid_self_dependency_snapshot()));

    assert!(matches!(
        result,
        Err(TaskSnapshotValidationError::SelfDependency { .. })
    ));
    assert_eq!(persist.collect_snapshot(), before);
}

#[test]
fn restore_captured_empty_clears_stale_state_via_canonical_empty_snapshot() {
    assert_state_clears_stale_tasks(SnapshotState::CapturedEmpty);
}

#[test]
fn restore_missing_clears_stale_state_via_canonical_empty_snapshot() {
    assert_state_clears_stale_tasks(SnapshotState::Missing);
}

fn assert_state_clears_stale_tasks(state: SnapshotState<TaskSnapshot>) {
    let wiring = wire_task();
    let access = wiring.access();
    let persist = wiring.persist();
    add_task(access.as_ref(), "stale");
    let adapter = TaskRestoreAdapter::new(Arc::clone(&persist));

    let prepared = adapter
        .prepare(&state)
        .expect("empty-compatible state must prepare as TaskSnapshot::empty");
    assert!(
        !access.list().is_empty(),
        "prepare must not mutate live state"
    );

    adapter.commit(prepared);
    assert_eq!(persist.collect_snapshot(), TaskSnapshot::empty());
    assert!(access.list().is_empty());
    assert!(access.list_batches().is_empty());
}

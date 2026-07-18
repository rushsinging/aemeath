//! Green tests for #888 crate-private `TaskStore` snapshot capture/install.
//!
//! Capture and candidate installation remain Task-BC internals: no `TaskPersist`
//! port or public restore capability is introduced here. #890 may publish and
//! wire the persistence boundary separately.

use super::{TaskAccess, TaskPersist, TaskStore};
use crate::business::{
    BatchCreateSpec, BatchId, TaskCreateSpec, TaskId, TaskPriority, TaskRevision, TaskSnapshot,
    TaskSnapshotValidationError, TaskStatus,
};

fn batch_spec(name: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(name.into()).unwrap()
}

fn task_spec(name: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(name.into(), String::new(), None, TaskPriority::Normal).unwrap()
}

/// One `TaskWireV2` entry rendered as raw JSON. Mirrors the wire shape
/// exactly; every field must be supplied so fixtures stay self-describing.
#[allow(clippy::too_many_arguments)]
fn task_json(
    id: &str,
    batch: &str,
    status: &str,
    created_at: u64,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
    blocked_by: &[&str],
    tags: &[&str],
) -> String {
    let started = started_at.map_or_else(|| "null".to_string(), |value| value.to_string());
    let completed = completed_at.map_or_else(|| "null".to_string(), |value| value.to_string());
    let blocked_by = blocked_by
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(",");
    let tags = tags
        .iter()
        .map(|tag| format!("\"{tag}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{id}","batch":"{batch}","subject":"t","description":"","active_form":null,"session_id":null,"tags":[{tags}],"blocked_by":[{blocked_by}],"status":"{status}","priority":"normal","created_at":{created_at},"updated_at":{updated_at},"started_at":{started},"completed_at":{completed}}}"#,
    )
}

/// One `BatchWireV2` entry rendered as raw JSON, mirroring the wire shape.
fn batch_json(id: &str, status: &str, created_at: u64) -> String {
    format!(
        r#"{{"id":"{id}","summary":"b","status":"{status}","created_at":{created_at},"last_active_turn":0,"silence_turns":0}}"#,
    )
}

/// Assembles a full V2 envelope from already-rendered task/batch fragments.
fn v2_bytes(
    revision: &str,
    tasks: &[String],
    next_task_id: &str,
    next_batch_id: &str,
    current_batch: Option<&str>,
    batches: &[String],
) -> Vec<u8> {
    let current = current_batch.map_or_else(|| "null".to_string(), |id| format!("\"{id}\""));
    format!(
        r#"{{"schema_version":2,"revision":"{revision}","tasks":[{tasks}],"next_task_id":"{next_task_id}","next_batch_id":"{next_batch_id}","current_batch":{current},"batches":[{batches}]}}"#,
        tasks = tasks.join(","),
        batches = batches.join(","),
    )
    .into_bytes()
}

#[test]
fn task_persist_contract_collect_prepare_commit_and_same_backing_views() {
    let store = TaskStore::new();
    let access: &dyn TaskAccess = &store;
    let persist: &dyn TaskPersist = &store;
    let batch = access.create_batch(batch_spec("批次"), 1).unwrap().value;
    let created = access.create_task(task_spec("任务"), 2).unwrap().value;

    let snapshot = persist.collect_snapshot();
    assert_eq!(snapshot.current_batch(), Some(batch.id()));
    assert_eq!(snapshot.tasks()[0].id(), created.id());

    let target = TaskStore::new();
    let target_access: &dyn TaskAccess = &target;
    target_access.create_batch(batch_spec("旧批次"), 3).unwrap();
    target_access.create_task(task_spec("旧任务"), 4).unwrap();
    let prepared = (&target as &dyn TaskPersist)
        .prepare_restore(&snapshot)
        .expect("合法 collect snapshot 应可 prepare");
    let unit: () = (&target as &dyn TaskPersist).commit_restore(prepared);
    assert_eq!(unit, ());

    assert_eq!(target_access.list(), access.list());
    assert_eq!(target_access.list_batches(), access.list_batches());
    assert_eq!((&target as &dyn TaskPersist).collect_snapshot(), snapshot);
}

#[test]
fn task_persist_prepare_failure_and_captured_empty_are_atomic() {
    let store = TaskStore::new();
    let access: &dyn TaskAccess = &store;
    access.create_batch(batch_spec("旧批次"), 1).unwrap();
    access.create_task(task_spec("旧任务"), 2).unwrap();
    let before = (&store as &dyn TaskPersist).collect_snapshot();

    let invalid = TaskSnapshot::decode(&v2_bytes(
        "1",
        &[
            task_json("1", "1", "pending", 1, 1, None, None, &[], &[]),
            task_json("1", "1", "pending", 1, 1, None, None, &[], &[]),
        ],
        "2",
        "2",
        Some("1"),
        &[batch_json("1", "active", 1)],
    ))
    .unwrap();
    assert!(matches!(
        (&store as &dyn TaskPersist).prepare_restore(&invalid),
        Err(TaskSnapshotValidationError::DuplicateTaskId { .. })
    ));
    assert_eq!((&store as &dyn TaskPersist).collect_snapshot(), before);

    let empty = TaskSnapshot::empty();
    let prepared = (&store as &dyn TaskPersist)
        .prepare_restore(&empty)
        .expect("captured empty 应合法");
    (&store as &dyn TaskPersist).commit_restore(prepared);
    assert!(access.list().is_empty());
    assert!(access.list_batches().is_empty());
    assert_eq!((&store as &dyn TaskPersist).collect_snapshot(), empty);
}

// ---- 1) mutated live state capture 保留 revision/counters/current/tasks/batches；encode V2/decode/validate ----

#[test]
fn capture_snapshot_preserves_mutated_live_state_and_round_trips_through_encode_decode_validate() {
    let store = TaskStore::new();
    let batch = store.create_batch(batch_spec("批次"), 0).unwrap().value;
    let a = store.create_task(task_spec("A"), 1).unwrap().value;
    let b = store.create_task(task_spec("B"), 2).unwrap().value;
    store.add_dependency(a.id(), b.id(), 3).unwrap();
    store.set_priority(a.id(), TaskPriority::High, 4).unwrap();
    store.transition(b.id(), TaskStatus::InProgress, 5).unwrap();
    store.transition(b.id(), TaskStatus::Completed, 6).unwrap();
    store.add_tag(a.id(), "urgent".into(), 7).unwrap();
    store.record_batch_turn(batch.id(), 3, true).unwrap();

    // The live backing exposed only for test-side assertions; this is the
    // ground truth the captured snapshot must match field-for-field.
    let live = store.state_snapshot();
    let captured = store.capture_snapshot();

    assert_eq!(captured.revision(), live.revision());
    assert_eq!(captured.next_task_id(), live.next_task_id());
    assert_eq!(captured.next_batch_id(), live.next_batch_id());
    assert_eq!(captured.current_batch(), live.current_batch());
    assert_eq!(captured.current_batch(), Some(batch.id()));

    let mut captured_task_ids: Vec<_> = captured.tasks().iter().map(|task| task.id()).collect();
    captured_task_ids.sort_unstable();
    assert_eq!(captured_task_ids, vec![a.id(), b.id()]);

    let captured_batch_ids: Vec<_> = captured.batches().iter().map(|entry| entry.id()).collect();
    assert_eq!(captured_batch_ids, vec![batch.id()]);

    let bytes = captured
        .encode()
        .expect("captured live snapshot must encode");
    let wire: serde_json::Value =
        serde_json::from_slice(&bytes).expect("encoded snapshot must be JSON");
    assert_eq!(wire["schema_version"], 2);

    let decoded = TaskSnapshot::decode(&bytes).expect("captured snapshot must decode");
    let prepared = decoded
        .prepare()
        .expect("a snapshot captured from live aggregate state must be installable");

    // The round trip must reconstruct byte-for-byte the same live backing,
    // including the derived reverse `blocks` index the aggregate already
    // maintained (revision/counters/current/tasks/batches all included).
    assert_eq!(prepared.candidate(), &live);
}

// ---- 2) Deleted task capture 过滤 ----

#[test]
fn capture_snapshot_filters_deleted_tasks_while_live_backing_still_holds_the_tombstone() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("批次"), 0).unwrap();
    let a = store.create_task(task_spec("A"), 1).unwrap().value;
    store.delete(a.id(), 2).unwrap();

    assert_eq!(store.stats().deleted, 1);
    let live = store.state_snapshot();
    assert!(
        live.tasks().contains_key(&a.id()),
        "the tombstone must remain in the live in-memory backing"
    );

    let captured = store.capture_snapshot();
    assert!(
        captured.tasks().is_empty(),
        "a persisted Deleted task must never appear in a capturable snapshot"
    );

    let bytes = captured.encode().expect("filtered snapshot must encode");
    let prepared = TaskSnapshot::decode(&bytes)
        .expect("filtered snapshot must decode")
        .prepare()
        .expect("a snapshot without any Deleted task must always validate");
    assert!(prepared.candidate().tasks().is_empty());
}

// ---- 3) valid candidate install 全量替换 old state/revision/counters/current ----

#[test]
fn install_snapshot_replaces_old_state_revision_counters_and_current_batch_wholesale() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("旧批次"), 0).unwrap();
    store.create_task(task_spec("旧任务"), 1).unwrap();
    let stale = store.state_snapshot();
    assert_eq!(stale.revision(), TaskRevision::new(2));

    let batch = batch_json("9", "active", 100);
    let task = task_json("9", "9", "pending", 100, 100, None, None, &[], &[]);
    let bytes = v2_bytes("9", &[task], "10", "10", Some("9"), &[batch]);
    let prepared = TaskSnapshot::decode(&bytes)
        .expect("fixture must decode")
        .prepare()
        .expect("fixture must validate");
    let expected = prepared.candidate().clone();
    assert_ne!(expected, stale);

    store.install_snapshot(prepared);

    assert_eq!(store.state_snapshot(), expected);
    assert_ne!(store.state_snapshot(), stale);
    assert_eq!(store.revision(), TaskRevision::new(9));
    assert_eq!(store.list().len(), 1);
    assert_eq!(store.list().first().unwrap().id(), TaskId::new(9));
    assert_eq!(store.list_batches().len(), 1);
    assert_eq!(store.list_batches().first().unwrap().id(), BatchId::new(9));
    assert_eq!(
        store.reminder_snapshot().current_batch,
        Some(BatchId::new(9))
    );
    assert!(store.get(TaskId::new(1)).is_none());
}

// ---- 4) invalid validate 不可安装且旧 state 完全不变 ----

#[test]
fn invalid_snapshot_never_produces_an_installable_candidate_and_leaves_old_state_untouched() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("批次"), 0).unwrap();
    let a = store.create_task(task_spec("A"), 1).unwrap().value;
    let baseline_state = store.state_snapshot();
    let baseline_snapshot = store.capture_snapshot();

    // Duplicate task ID: decode() alone only enforces wire-format ID shape,
    // so this still decodes; only validate() enforces aggregate-level
    // uniqueness. There is no way to obtain a `PreparedTaskRestore` from it,
    // so `install_snapshot` can never be reached for a rejected snapshot.
    let batch = batch_json("1", "active", 100);
    let dup_a = task_json("1", "1", "pending", 100, 100, None, None, &[], &[]);
    let dup_b = task_json("1", "1", "pending", 100, 100, None, None, &[], &[]);
    let bytes = v2_bytes("1", &[dup_a, dup_b], "2", "2", Some("1"), &[batch]);

    let error = TaskSnapshot::decode(&bytes)
        .expect("duplicate IDs are still well-formed wire data")
        .prepare()
        .expect_err("duplicate task IDs must be rejected by validate()");
    assert!(matches!(
        error,
        TaskSnapshotValidationError::DuplicateTaskId { id } if id == TaskId::new(1)
    ));

    assert_eq!(store.state_snapshot(), baseline_state);
    assert_eq!(store.capture_snapshot(), baseline_snapshot);
    assert_eq!(store.get(a.id()), Some(a));
}

// ---- 5) restored reverse blocks 一致（install 之后再次 capture round trip 仍一致） ----

#[test]
fn install_snapshot_restores_reverse_blocks_consistently_with_a_later_capture_round_trip() {
    let batch = batch_json("1", "active", 100);
    let task_a = task_json("1", "1", "pending", 100, 100, None, None, &["2"], &[]);
    let task_b = task_json("2", "1", "pending", 100, 100, None, None, &[], &[]);
    let bytes = v2_bytes("2", &[task_a, task_b], "3", "2", Some("1"), &[batch]);

    let prepared = TaskSnapshot::decode(&bytes)
        .expect("fixture must decode")
        .prepare()
        .expect("fixture must validate");
    let expected_blocks = prepared
        .candidate()
        .tasks()
        .get(&TaskId::new(2))
        .expect("task 2 must exist in the candidate")
        .blocks()
        .to_vec();
    assert_eq!(expected_blocks, vec![TaskId::new(1)]);

    let store = TaskStore::new();
    store.install_snapshot(prepared);

    assert_eq!(
        store.get(TaskId::new(2)).unwrap().blocks(),
        [TaskId::new(1)]
    );
    assert!(store.is_blocked(TaskId::new(1)).unwrap());

    // A subsequent capture/encode/decode/validate cycle over the freshly
    // installed live state must reconstruct the exact same reverse index; no
    // command mutated anything in between install and recapture.
    let recaptured = store.capture_snapshot();
    let bytes = recaptured
        .encode()
        .expect("recaptured snapshot must encode");
    let reprepared = TaskSnapshot::decode(&bytes)
        .expect("recaptured snapshot must decode")
        .prepare()
        .expect("a snapshot captured right after install must always validate");
    assert_eq!(
        reprepared
            .candidate()
            .tasks()
            .get(&TaskId::new(2))
            .expect("task 2 must exist after the round trip")
            .blocks(),
        [TaskId::new(1)]
    );
}

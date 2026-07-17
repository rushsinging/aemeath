//! Red tests for #888 aggregate `TaskSnapshot` validation.
//!
//! This module intentionally exercises an API that does **not** exist yet:
//! Public `TaskSnapshot::validate() -> Result<(), TaskSnapshotValidationError>`
//! checks aggregate validity; the final consistency test uses crate-private
//! `prepare()` to inspect the reconstructed candidate state.
//! Per #888 scope, decode (`TaskSnapshot::decode`) only enforces *wire format*
//! rules (typed ID string shape, non-zero IDs on the V2 path, JSON schema
//! version). `validate()` is the separate, pure, side-effect-free layer that
//! checks *aggregate* invariants on an already-decoded snapshot: duplicate
//! entity IDs, dangling/duplicate/cyclic/cross-batch dependency edges,
//! persisted tombstones, Batch/current_batch consistency, next-ID counters
//! and created/updated/started/completed timestamp legality.
//!
//! `cargo test -p task snapshot_validation` is expected to **fail to
//! compile** until the validator and its error type land: this is the
//! documented Red state, not a test-logic bug. Do not add a validator
//! implementation or any production code while resolving this file.

use super::{BatchId, TaskId, TaskRevision, TaskSnapshot, TaskSnapshotValidationError};

fn decode(bytes: &[u8]) -> TaskSnapshot {
    TaskSnapshot::decode(bytes).expect("fixture must decode")
}

/// One `TaskWireV2` entry. Every field mirrors `TaskWireV2` exactly so a
/// fixture only needs to override the properties relevant to the invariant
/// under test.
struct TaskFixture<'a> {
    id: &'a str,
    batch: &'a str,
    status: &'a str,
    created_at: u64,
    updated_at: u64,
    started_at: Option<u64>,
    completed_at: Option<u64>,
    blocked_by: &'a [&'a str],
}

impl<'a> TaskFixture<'a> {
    fn json(&self) -> String {
        let started = self
            .started_at
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        let completed = self
            .completed_at
            .map_or_else(|| "null".to_string(), |value| value.to_string());
        let blocked_by = self
            .blocked_by
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"id":"{id}","batch":"{batch}","subject":"t","description":"","active_form":null,"session_id":null,"tags":[],"blocked_by":[{blocked_by}],"status":"{status}","priority":"normal","created_at":{created_at},"updated_at":{updated_at},"started_at":{started},"completed_at":{completed}}}"#,
            id = self.id,
            batch = self.batch,
            status = self.status,
            created_at = self.created_at,
            updated_at = self.updated_at,
        )
    }
}

/// One `BatchWireV2` entry, mirroring `BatchWireV2` exactly.
struct BatchFixture<'a> {
    id: &'a str,
    status: &'a str,
    created_at: u64,
}

impl<'a> BatchFixture<'a> {
    fn json(&self) -> String {
        format!(
            r#"{{"id":"{id}","summary":"b","status":"{status}","created_at":{created_at},"last_active_turn":0,"silence_turns":0}}"#,
            id = self.id,
            status = self.status,
            created_at = self.created_at,
        )
    }
}

/// Assembles a full V2 envelope from already-rendered task/batch fragments.
fn snapshot_bytes(
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

/// A single `Active` batch and a single otherwise-consistent `Pending` task
/// referencing it; every test below starts from an equivalent baseline and
/// mutates exactly one property so only the invariant under test can fail.
fn baseline_batch() -> BatchFixture<'static> {
    BatchFixture {
        id: "1",
        status: "active",
        created_at: 100,
    }
}

fn baseline_pending_task<'a>(
    id: &'a str,
    batch: &'a str,
    blocked_by: &'a [&'a str],
) -> TaskFixture<'a> {
    TaskFixture {
        id,
        batch,
        status: "pending",
        created_at: 100,
        updated_at: 100,
        started_at: None,
        completed_at: None,
        blocked_by,
    }
}

#[test]
fn snapshot_validate_rejects_duplicate_task_id() {
    let batch = baseline_batch().json();
    let task_a = baseline_pending_task("1", "1", &[]).json();
    let task_b = baseline_pending_task("1", "1", &[]).json();
    let bytes = snapshot_bytes("0", &[task_a, task_b], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("duplicate task ID must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::DuplicateTaskId { id } if id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_duplicate_batch_id() {
    let batch_a = BatchFixture {
        id: "1",
        status: "paused",
        created_at: 100,
    }
    .json();
    let batch_b = BatchFixture {
        id: "1",
        status: "paused",
        created_at: 100,
    }
    .json();
    let bytes = snapshot_bytes("0", &[], "1", "2", None, &[batch_a, batch_b]);

    let error = decode(&bytes)
        .validate()
        .expect_err("duplicate batch ID must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::DuplicateBatchId { id } if id == BatchId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_zero_entity_id_surfaced_by_legacy_v1_upgrade() {
    // V2 decode already rejects a zero ID at the wire-format layer (see
    // `snapshot_v2_rejects_numeric_mixed_and_zero_id_representations` in
    // `snapshot_tests.rs`), and V1 Task IDs go through the same non-zero
    // `parse_id` check. Legacy V1 `BatchWireV1::id` is a bare `u64` with no
    // such check, so it is the only remaining route through which a
    // structurally-valid `TaskSnapshot` can carry a zero entity ID — this is
    // exactly the aggregate-level defect `validate()` must still catch.
    let legacy = br#"{
      "tasks": [],
      "next_id": 1,
      "current_batch": 0,
      "batches": [{
        "id": 0,
        "summary": "legacy",
        "status": "archived",
        "created_at": 10,
        "last_active_turn": 0,
        "silence_turns": 0
      }]
    }"#;

    let error = decode(legacy)
        .validate()
        .expect_err("zero batch ID entity must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::ZeroBatchId { id } if id == BatchId::new(0)
    ));
}

#[test]
fn snapshot_validate_rejects_persisted_deleted_task() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "deleted",
        created_at: 100,
        updated_at: 110,
        started_at: Some(100),
        completed_at: None,
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("persisted Deleted tombstone must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::PersistedDeletedTask { id } if id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_missing_batch_reference() {
    let batch = baseline_batch().json();
    let task = baseline_pending_task("1", "9", &[]).json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("task referencing a non-existent batch must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidBatchReference { task_id, batch_id }
            if task_id == TaskId::new(1) && batch_id == BatchId::new(9)
    ));
}

#[test]
fn snapshot_validate_rejects_dangling_dependency() {
    let batch = baseline_batch().json();
    let task = baseline_pending_task("1", "1", &["99"]).json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("dependency on a non-existent task must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::DanglingDependency { task_id, dependency_id }
            if task_id == TaskId::new(1) && dependency_id == TaskId::new(99)
    ));
}

#[test]
fn snapshot_validate_rejects_self_dependency() {
    let batch = baseline_batch().json();
    let task = baseline_pending_task("1", "1", &["1"]).json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("a task blocked by itself must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::SelfDependency { task_id } if task_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_indirect_dependency_cycle() {
    let batch = baseline_batch().json();
    // 1 -> 3 -> 2 -> 1: no self-loop, but a three-node cycle.
    let task_1 = baseline_pending_task("1", "1", &["3"]).json();
    let task_2 = baseline_pending_task("2", "1", &["1"]).json();
    let task_3 = baseline_pending_task("3", "1", &["2"]).json();
    let bytes = snapshot_bytes(
        "0",
        &[task_1, task_2, task_3],
        "4",
        "2",
        Some("1"),
        &[batch],
    );

    let error = decode(&bytes)
        .validate()
        .expect_err("an indirect dependency cycle must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::DependencyCycle
    ));
}

#[test]
fn snapshot_validate_rejects_cross_batch_dependency() {
    let batch_1 = baseline_batch().json();
    let batch_2 = BatchFixture {
        id: "2",
        status: "archived",
        created_at: 100,
    }
    .json();
    let task_1 = baseline_pending_task("1", "1", &[]).json();
    let task_2 = baseline_pending_task("2", "2", &["1"]).json();
    let bytes = snapshot_bytes(
        "0",
        &[task_1, task_2],
        "3",
        "3",
        Some("1"),
        &[batch_1, batch_2],
    );

    let error = decode(&bytes)
        .validate()
        .expect_err("a dependency edge crossing batches must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::CrossBatchDependency { task_id, blocked_by_id }
            if task_id == TaskId::new(2) && blocked_by_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_duplicate_blocked_by_reference() {
    let batch = baseline_batch().json();
    let task_1 = baseline_pending_task("1", "1", &[]).json();
    let task_2 = baseline_pending_task("2", "1", &["1", "1"]).json();
    let bytes = snapshot_bytes("0", &[task_1, task_2], "3", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("a repeated blocked_by entry must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::DuplicateDependencyReference { task_id, dependency_id }
            if task_id == TaskId::new(2) && dependency_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_multiple_active_batches() {
    let batch_1 = baseline_batch().json();
    let batch_2 = BatchFixture {
        id: "2",
        status: "active",
        created_at: 100,
    }
    .json();
    let bytes = snapshot_bytes("0", &[], "1", "3", Some("1"), &[batch_1, batch_2]);

    let error = decode(&bytes)
        .validate()
        .expect_err("more than one Active batch must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::MultipleActiveBatches { first, second }
            if [first.get(), second.get()].iter().all(|id| [1, 2].contains(id))
                && first != second
    ));
}

#[test]
fn snapshot_validate_rejects_current_batch_missing_reference() {
    let batch = BatchFixture {
        id: "1",
        status: "archived",
        created_at: 100,
    }
    .json();
    let bytes = snapshot_bytes("0", &[], "1", "2", Some("9"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("current_batch pointing at a non-existent batch must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidCurrentBatch { batch_id } if batch_id == BatchId::new(9)
    ));
}

#[test]
fn snapshot_validate_rejects_current_batch_pointing_to_non_active_batch() {
    let batch = BatchFixture {
        id: "1",
        status: "archived",
        created_at: 100,
    }
    .json();
    let bytes = snapshot_bytes("0", &[], "1", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("current_batch pointing at a non-Active batch must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidCurrentBatch { batch_id } if batch_id == BatchId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_current_batch_mismatch_with_actual_active_batch() {
    let batch = baseline_batch().json();
    // An Active batch exists, but current_batch does not point to it.
    let bytes = snapshot_bytes("0", &[], "1", "2", None, &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("current_batch missing while an Active batch exists must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::CurrentBatchMismatch { current: None, active }
            if active == BatchId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_next_task_id_not_greater_than_max_task_id() {
    let batch = baseline_batch().json();
    let task = baseline_pending_task("5", "1", &[]).json();
    // next_task_id equals the max existing task ID instead of exceeding it.
    let bytes = snapshot_bytes("0", &[task], "5", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("next_task_id <= max existing task ID must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidNextTaskId
    ));
}

#[test]
fn snapshot_validate_rejects_next_batch_id_not_greater_than_max_batch_id() {
    let batch = BatchFixture {
        id: "3",
        status: "active",
        created_at: 100,
    }
    .json();
    // next_batch_id equals the max existing batch ID instead of exceeding it.
    let bytes = snapshot_bytes("0", &[], "1", "3", Some("3"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("next_batch_id <= max existing batch ID must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidNextBatchId
    ));
}

#[test]
fn snapshot_validate_rejects_updated_at_before_created_at() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "pending",
        created_at: 100,
        updated_at: 50,
        started_at: None,
        completed_at: None,
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("updated_at before created_at must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_started_at_before_created_at() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "in_progress",
        created_at: 100,
        updated_at: 100,
        started_at: Some(50),
        completed_at: None,
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("started_at before created_at must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_completed_at_before_started_at() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "completed",
        created_at: 100,
        updated_at: 130,
        started_at: Some(120),
        completed_at: Some(110),
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("completed_at before started_at must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_completed_status_missing_completed_at() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "completed",
        created_at: 100,
        updated_at: 120,
        started_at: Some(110),
        completed_at: None,
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("Completed status without completed_at must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

#[test]
fn snapshot_validate_rejects_pending_status_with_started_at() {
    let batch = baseline_batch().json();
    let task = TaskFixture {
        id: "1",
        batch: "1",
        status: "pending",
        created_at: 100,
        updated_at: 100,
        started_at: Some(100),
        completed_at: None,
        blocked_by: &[],
    }
    .json();
    let bytes = snapshot_bytes("0", &[task], "2", "2", Some("1"), &[batch]);

    let error = decode(&bytes)
        .validate()
        .expect_err("Pending status carrying started_at must be rejected");

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

/// Red: #888 legacy V1 `in_progress`/`completed` tasks recorded before
/// `started_at`/`completed_at` existed on the wire must decode *and validate*
/// successfully, with the missing timestamps deterministically derived from
/// `updated_at` rather than rejected outright.
///
/// `TaskWireV1::started_at`/`completed_at` are `#[serde(default)]`, so a
/// legacy record that never persisted them decodes today with both fields as
/// `None`. `valid_task_timestamps` in `snapshot.rs` then requires
/// `InProgress` to carry `started_at.is_some()` and rejects the task with
/// `InvalidTaskTimestamps` -- confirmed empirically: `TaskSnapshot::decode`
/// on the fixture below succeeds, but `.prepare()` currently returns
/// `Err(InvalidTaskTimestamps { task_id: TaskId(1) })`, so the
/// `.expect(..)` below currently panics (Red). The desired fix derives
/// `started_at = updated_at` and leaves `completed_at = None` for `InProgress`
/// so the record round-trips into a valid V2 aggregate instead of being
/// dropped -- a decode/prepare-time upgrade, not a change to `validate`'s
/// invariants themselves.
#[test]
fn snapshot_validate_derives_in_progress_started_at_from_updated_at_for_legacy_v1_task_missing_timestamps(
) {
    let legacy = br#"{
      "tasks": [
        {"id":"1","batch":1,"subject":"t","status":"in_progress","created_at":100,"updated_at":150}
      ],
      "next_id": 2,
      "current_batch": 1,
      "batches": [{"id":1,"summary":"legacy","status":"active","created_at":50,"last_active_turn":1,"silence_turns":0}]
    }"#;

    let prepared = decode(legacy).prepare().expect(
        "a legacy V1 in_progress task missing started_at/completed_at must validate via deterministic derivation",
    );
    let task = prepared
        .candidate()
        .tasks()
        .get(&TaskId::new(1))
        .expect("derived task must be installed");

    assert_eq!(
        task.started_at(),
        Some(150),
        "InProgress started_at must be derived from updated_at"
    );
    assert_eq!(
        task.completed_at(),
        None,
        "InProgress completed_at must remain absent"
    );
}

/// Red: companion to the `InProgress` derivation test above for legacy V1
/// `Completed` tasks. Confirmed empirically: `.prepare()` on the fixture
/// below currently returns
/// `Err(InvalidTaskTimestamps { task_id: TaskId(1) })` because both
/// `started_at` and `completed_at` decode as `None`, so the `.expect(..)`
/// below currently panics (Red). The desired fix derives both
/// `started_at = updated_at` *and* `completed_at = updated_at` for
/// `Completed`.
#[test]
fn snapshot_validate_derives_completed_started_and_completed_at_from_updated_at_for_legacy_v1_task_missing_timestamps(
) {
    let legacy = br#"{
      "tasks": [
        {"id":"1","batch":1,"subject":"t","status":"completed","created_at":100,"updated_at":170}
      ],
      "next_id": 2,
      "current_batch": 1,
      "batches": [{"id":1,"summary":"legacy","status":"active","created_at":50,"last_active_turn":1,"silence_turns":0}]
    }"#;

    let prepared = decode(legacy).prepare().expect(
        "a legacy V1 completed task missing started_at/completed_at must validate via deterministic derivation",
    );
    let task = prepared
        .candidate()
        .tasks()
        .get(&TaskId::new(1))
        .expect("derived task must be installed");

    assert_eq!(
        task.started_at(),
        Some(170),
        "Completed started_at must be derived from updated_at"
    );
    assert_eq!(
        task.completed_at(),
        Some(170),
        "Completed completed_at must be derived from updated_at"
    );
}

/// Guard (currently Green, not Red): unlike the two derivation tests above,
/// a legacy V1 `Completed` record whose `created_at` exceeds its
/// `updated_at` must still be rejected by `validate()` even once timestamp
/// derivation for missing `started_at`/`completed_at` lands. `updated_at <
/// created_at` is checked unconditionally in `valid_task_timestamps` before
/// any per-status derivation could apply, so this fixture already returns
/// `Err(InvalidTaskTimestamps { task_id: TaskId(1) })` today -- confirmed
/// empirically -- and must continue to do so after the derivation fix above
/// is implemented. This test locks that invariant so a future, naive
/// "derive first, validate second" implementation cannot accidentally paper
/// over a corrupt legacy record by deriving consistent-looking timestamps
/// from an already-inconsistent `created_at`/`updated_at` pair.
#[test]
fn snapshot_validate_rejects_legacy_v1_completed_task_when_created_at_exceeds_updated_at_even_with_missing_timestamps(
) {
    let legacy = br#"{
      "tasks": [
        {"id":"1","batch":1,"subject":"t","status":"completed","created_at":200,"updated_at":100}
      ],
      "next_id": 2,
      "current_batch": 1,
      "batches": [{"id":1,"summary":"legacy","status":"active","created_at":50,"last_active_turn":1,"silence_turns":0}]
    }"#;

    let error = decode(legacy).validate().expect_err(
        "created_at after updated_at must still be rejected regardless of timestamp derivation",
    );

    assert!(matches!(
        error,
        TaskSnapshotValidationError::InvalidTaskTimestamps { task_id } if task_id == TaskId::new(1)
    ));
}

/// Red (documented, scaled down for safety -- see comment below): a long
/// linear dependency chain must validate successfully without stack
/// overflow, regardless of the order tasks are listed in the wire array.
///
/// `dependency_graph_has_cycle` in `snapshot.rs` performs classic recursive,
/// color-marking DFS: `visit(index, ..)` recurses once per hop along
/// `blocked_by` before returning. Because the *outer* loop
/// `(0..tasks.len()).any(|index| visit(index, ..))` starts a fresh DFS from
/// every array index in order, a chain listed in **ascending** dependency
/// order (task 1, then 2 which depends on 1, then 3 which depends on 2, ..)
/// never recurses deeper than one hop: by the time `visit` reaches task k,
/// task k-1 was already colored black by an earlier outer-loop iteration.
/// But a chain listed in the *opposite* order -- last-created task first,
/// exactly how a naive "iterate tasks() in insertion/HashMap order" snapshot
/// capture could plausibly emit it -- forces the very first outer-loop call
/// to recurse the *entire* chain depth before any node is marked, with no
/// tail-call elimination (the recursive call sits inside a `for` loop, not
/// in tail position).
///
/// Confirmed empirically on this machine (aarch64 macOS, debug profile,
/// default 2 MiB test-thread stack, no `RUST_MIN_STACK` override): a
/// **20,000**-task reverse-ordered chain reliably aborts the entire test
/// process with `fatal runtime error: stack overflow` (SIGABRT) -- not a
/// catchable panic, an unrecoverable process abort that would take the rest
/// of the `cargo test -p task` binary down with it. A **15,000**-task chain
/// reproduces the same abort; a **10,000**-task chain does not. Per this
/// task's own explicit fallback ("若危险可仅写 5k 并记录"), this committed
/// test therefore exercises only **5,000** reverse-ordered tasks -- safely
/// below the observed crash threshold on this machine, but still large
/// enough to exercise realistic aggregate sizes -- and currently passes
/// (Green) rather than demonstrating the crash directly. The crash itself
/// is recorded here as a documented finding, not asserted in-process,
/// because a stack overflow cannot be caught by `#[should_panic]` or any
/// `Result`-based assertion: it aborts the process before test-harness
/// unwinding can run. Fixing the underlying defect requires converting
/// `dependency_graph_has_cycle` to an explicit-stack (heap-allocated)
/// iterative traversal so validation depth no longer depends on native call
/// stack depth or on wire task ordering -- production code, out of scope
/// for this commit.
#[test]
fn snapshot_validate_accepts_a_long_reverse_ordered_dependency_chain_without_stack_overflow() {
    const CHAIN_LEN: u64 = 5_000;

    let batch = baseline_batch().json();
    // Emit tasks from the *last* id down to `1` so task 1's dependency chain
    // is entirely unresolved (uncolored) when the outer validation loop's
    // very first DFS call starts at array index 0 (task `CHAIN_LEN`).
    let tasks: Vec<String> = (1..=CHAIN_LEN)
        .rev()
        .map(|id| {
            let id_string = id.to_string();
            if id > 1 {
                let dependency = (id - 1).to_string();
                baseline_pending_task(&id_string, "1", &[dependency.as_str()]).json()
            } else {
                baseline_pending_task(&id_string, "1", &[]).json()
            }
        })
        .collect();
    let next_task_id = (CHAIN_LEN + 1).to_string();
    let bytes = snapshot_bytes("0", &tasks, &next_task_id, "2", Some("1"), &[batch]);

    let result = decode(&bytes).validate();

    assert!(
        result.is_ok(),
        "a {CHAIN_LEN}-task reverse-ordered dependency chain must validate successfully: {result:?}"
    );
}

#[test]
fn snapshot_validate_accepts_fully_consistent_snapshot_and_installs_store_state() {
    let batch = baseline_batch().json();
    let task = baseline_pending_task("1", "1", &[]).json();
    let bytes = snapshot_bytes("5", &[task], "2", "2", Some("1"), &[batch]);

    let prepared = decode(&bytes)
        .prepare()
        .expect("a fully consistent snapshot must validate");
    let state = prepared.candidate();

    assert_eq!(state.tasks().len(), 1);
    assert!(state.tasks().contains_key(&TaskId::new(1)));
    assert_eq!(state.batches().len(), 1);
    assert!(state.batches().contains_key(&BatchId::new(1)));
    assert_eq!(state.current_batch(), Some(BatchId::new(1)));
    assert_eq!(state.revision(), TaskRevision::new(5));
    assert_eq!(state.next_task_id(), TaskId::new(2));
    assert_eq!(state.next_batch_id(), BatchId::new(2));
}

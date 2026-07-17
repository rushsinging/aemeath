use crate::{
    Batch, BatchCreateSpec, BatchId, BatchStatus, Task, TaskAccess, TaskCommandError,
    TaskCreateSpec, TaskEvent, TaskPriority, TaskRevision, TaskStatus,
};

fn batch_spec(subject: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(subject.to_owned()).expect("valid batch spec")
}

fn task_spec(subject: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(
        subject.to_owned(),
        String::new(),
        None,
        TaskPriority::Normal,
    )
    .expect("valid task spec")
}

/// Every read the `TaskAccess` port exposes that a failed or no-op command
/// MUST leave byte-for-byte untouched: the authoritative revision plus both
/// deterministic list projections. Comparing this triple before/after a
/// rejected or idempotent command is a `dyn TaskAccess`-safe substitute for
/// inspecting a concrete backing snapshot.
fn observable(access: &dyn TaskAccess) -> (TaskRevision, Vec<Task>, Vec<Batch>) {
    (access.revision(), access.list(), access.list_batches())
}

/// Pre-overflow fixtures for the three independent exhaustion scenarios.
///
/// Each scenario needs its own starting shape (an already-seeded Batch ID
/// counter, an already-seeded Task ID counter with no Batch yet, or an
/// already-maxed revision) so the contract stays agnostic to *how* a
/// concrete `TaskAccess` implementation constructs that starting state.
pub(super) struct TaskAccessOverflowFixtures<'a> {
    pub revision_exhausted: &'a dyn TaskAccess,
    pub batch_id_exhausted: &'a dyn TaskAccess,
    pub task_id_exhausted: &'a dyn TaskAccess,
}

/// Reusable behavioral contract for every `TaskAccess` implementation.
pub(super) fn assert_task_access_contract(
    access: &dyn TaskAccess,
    overflow: TaskAccessOverflowFixtures<'_>,
) {
    // Empty backing is revision zero.
    assert_eq!(access.revision(), TaskRevision::new(0));

    // A failed command is atomic and does not advance revision.
    assert_eq!(
        access.create_task(task_spec("orphan"), 1),
        Err(TaskCommandError::NoActiveBatch)
    );
    assert_eq!(access.revision(), TaskRevision::new(0));
    assert!(access.list().is_empty());

    // Each real write advances exactly once, and the returned revision belongs
    // to the same transaction as its value/events.
    let batch = access
        .create_batch(batch_spec("batch"), 2)
        .expect("batch creation succeeds");
    assert_eq!(batch.revision(), Some(TaskRevision::new(1)));
    assert_eq!(access.revision(), TaskRevision::new(1));
    assert_eq!(access.list_batches(), vec![batch.value.clone()]);

    let second = access
        .create_task(task_spec("second"), 3)
        .expect("task creation succeeds");
    assert_eq!(second.revision(), Some(TaskRevision::new(2)));
    assert_eq!(access.revision(), TaskRevision::new(2));
    assert_eq!(access.get(second.value.id()), Some(second.value.clone()));

    let first = access
        .create_task(task_spec("first"), 4)
        .expect("task creation succeeds");
    assert_eq!(first.revision(), Some(TaskRevision::new(3)));
    assert_eq!(access.revision(), TaskRevision::new(3));

    // Queries are deterministic and cannot advance revision. In particular,
    // list order is typed-ID order rather than HashMap iteration order.
    let before_queries = access.revision();
    let expected = vec![second.value.clone(), first.value.clone()];
    assert_eq!(access.list(), expected);
    assert_eq!(access.list(), expected);
    assert_eq!(access.list_batches(), access.list_batches());
    assert_eq!(access.stats(), access.stats());
    assert_eq!(access.reminder_snapshot(), access.reminder_snapshot());
    assert_eq!(access.lifecycle_snapshot(5), access.lifecycle_snapshot(5));
    assert!(!access.is_blocked(first.value.id()).expect("known task"));
    assert!(!access.would_create_cycle(first.value.id(), second.value.id()));
    assert_eq!(access.revision(), before_queries);

    // An idempotent successful no-op has no commit revision or events.
    let no_op = access
        .set_priority(
            first.value.id(),
            first.value.priority(),
            first.value.updated_at() + 1,
        )
        .expect("idempotent update succeeds");
    assert_eq!(no_op.revision(), None);
    assert!(no_op.events.is_empty());
    assert_eq!(access.revision(), before_queries);

    // ---- A second concurrent Batch is rejected atomically: no new Batch is
    // admitted and the active-conflict failure never reserves a revision ----
    let revision_before_conflict = access.revision();
    let batches_before_conflict = access.list_batches();
    assert_eq!(
        access.create_batch(batch_spec("conflict"), 5),
        Err(TaskCommandError::ActiveBatchConflict {
            active: batch.value.id(),
            requested: BatchId::new(batch.value.id().get() + 1),
        })
    );
    assert_eq!(access.revision(), revision_before_conflict);
    assert_eq!(access.list_batches(), batches_before_conflict);

    // ---- Multi-entity dependency add/remove and delete: revision, events,
    // and every touched entity's edges stay consistent within one commit ----
    let alpha = access
        .create_task(task_spec("alpha"), 6)
        .expect("task creation succeeds")
        .value;
    let beta = access
        .create_task(task_spec("beta"), 7)
        .expect("task creation succeeds")
        .value;
    let gamma = access
        .create_task(task_spec("gamma"), 8)
        .expect("task creation succeeds")
        .value;
    let delta = access
        .create_task(task_spec("delta"), 9)
        .expect("task creation succeeds")
        .value;

    // alpha depends on beta; beta and delta both depend on gamma.
    let revision_before_edges = access.revision();
    let added_ab = access
        .add_dependency(alpha.id(), beta.id(), 10)
        .expect("edge admitted");
    assert_eq!(
        added_ab.revision(),
        Some(TaskRevision::new(revision_before_edges.get() + 1))
    );
    assert_eq!(access.get(alpha.id()).unwrap().blocked_by(), &[beta.id()]);
    assert_eq!(access.get(beta.id()).unwrap().blocks(), &[alpha.id()]);

    let added_bg = access
        .add_dependency(beta.id(), gamma.id(), 11)
        .expect("edge admitted");
    assert_eq!(
        added_bg.revision(),
        Some(TaskRevision::new(revision_before_edges.get() + 2))
    );
    let added_dg = access
        .add_dependency(delta.id(), gamma.id(), 12)
        .expect("edge admitted");
    assert_eq!(
        added_dg.revision(),
        Some(TaskRevision::new(revision_before_edges.get() + 3))
    );

    assert!(access.is_blocked(alpha.id()).expect("known task"));
    assert!(access.is_blocked(beta.id()).expect("known task"));
    assert!(access.is_blocked(delta.id()).expect("known task"));
    assert!(!access.is_blocked(gamma.id()).expect("known task"));

    // Re-adding the same edge is an idempotent no-op.
    let revision_before_duplicate_edge = access.revision();
    let duplicate_edge = access
        .add_dependency(alpha.id(), beta.id(), 13)
        .expect("idempotent edge succeeds");
    assert_eq!(duplicate_edge.revision(), None);
    assert!(duplicate_edge.events.is_empty());
    assert_eq!(access.revision(), revision_before_duplicate_edge);

    // A self-cycle is rejected atomically without touching any entity.
    let observable_before_self_cycle = observable(access);
    assert_eq!(
        access.add_dependency(alpha.id(), alpha.id(), 14),
        Err(TaskCommandError::DependencyCycle {
            task_id: alpha.id(),
            blocked_by_id: alpha.id(),
        })
    );
    assert_eq!(observable(access), observable_before_self_cycle);

    // An indirect cycle (gamma -> alpha would close alpha -> beta -> gamma)
    // is rejected the same way; the advisory query agrees beforehand.
    assert!(access.would_create_cycle(gamma.id(), alpha.id()));
    let observable_before_indirect_cycle = observable(access);
    assert_eq!(
        access.add_dependency(gamma.id(), alpha.id(), 15),
        Err(TaskCommandError::DependencyCycle {
            task_id: gamma.id(),
            blocked_by_id: alpha.id(),
        })
    );
    assert_eq!(observable(access), observable_before_indirect_cycle);

    // Deleting `beta` (which both depends on gamma and is depended on by
    // alpha) clears both edge directions for its direct neighbours in the
    // same commit, while `delta`'s unrelated edge to gamma is untouched.
    let revision_before_delete = access.revision();
    let deleted = access.delete(beta.id(), 16).expect("delete succeeds");
    assert_eq!(
        deleted.revision(),
        Some(TaskRevision::new(revision_before_delete.get() + 1))
    );
    assert_eq!(
        deleted.events,
        vec![TaskEvent::TaskDeleted { task_id: beta.id() }]
    );
    assert_eq!(deleted.value.status(), TaskStatus::Deleted);
    assert!(deleted.value.blocked_by().is_empty());
    assert!(deleted.value.blocks().is_empty());
    assert!(access.get(alpha.id()).unwrap().blocked_by().is_empty());
    assert!(!access
        .get(gamma.id())
        .unwrap()
        .blocks()
        .contains(&beta.id()));
    assert_eq!(access.get(delta.id()).unwrap().blocked_by(), &[gamma.id()]);
    assert!(!access.list().contains(&deleted.value));

    // ---- Task status transition: legal path plus an atomic illegal
    // transition failure that must not disturb any read model ----
    let revision_before_transition = access.revision();
    let started = access
        .transition(gamma.id(), TaskStatus::InProgress, 17)
        .expect("legal transition");
    assert_eq!(
        started.revision(),
        Some(TaskRevision::new(revision_before_transition.get() + 1))
    );
    let completed = access
        .transition(gamma.id(), TaskStatus::Completed, 18)
        .expect("legal transition");
    assert_eq!(
        completed.revision(),
        Some(TaskRevision::new(revision_before_transition.get() + 2))
    );
    assert_eq!(completed.value.status(), TaskStatus::Completed);
    // Once its sole dependency completes, `delta` is no longer blocked.
    assert!(!access.is_blocked(delta.id()).expect("known task"));

    let observable_before_illegal_transition = observable(access);
    assert_eq!(
        access.transition(gamma.id(), TaskStatus::InProgress, 19),
        Err(TaskCommandError::IllegalTransition {
            from: TaskStatus::Completed,
            to: TaskStatus::InProgress,
        })
    );
    assert_eq!(observable(access), observable_before_illegal_transition);
    assert_eq!(
        access.transition(gamma.id(), TaskStatus::Deleted, 20),
        Err(TaskCommandError::DeletedOnlyViaDelete)
    );
    assert_eq!(observable(access), observable_before_illegal_transition);

    // ---- Tag commands: add/remove with an idempotent no-op both ways, and
    // rejection on an already-deleted Task ----
    let revision_before_tag = access.revision();
    let tagged = access
        .add_tag(alpha.id(), "urgent".to_owned(), 21)
        .expect("tag add succeeds");
    assert_eq!(
        tagged.revision(),
        Some(TaskRevision::new(revision_before_tag.get() + 1))
    );
    assert_eq!(tagged.value.tags(), ["urgent".to_owned()]);

    let duplicate_tag = access
        .add_tag(alpha.id(), "urgent".to_owned(), 22)
        .expect("idempotent tag succeeds");
    assert_eq!(duplicate_tag.revision(), None);
    assert!(duplicate_tag.events.is_empty());

    let untagged = access
        .remove_tag(alpha.id(), "urgent", 23)
        .expect("tag remove succeeds");
    assert_eq!(
        untagged.revision(),
        Some(TaskRevision::new(revision_before_tag.get() + 2))
    );
    assert!(untagged.value.tags().is_empty());

    let absent_tag = access
        .remove_tag(alpha.id(), "urgent", 24)
        .expect("idempotent removal succeeds");
    assert_eq!(absent_tag.revision(), None);
    assert!(absent_tag.events.is_empty());

    let observable_before_deleted_tag = observable(access);
    assert_eq!(
        access.add_tag(beta.id(), "late".to_owned(), 25),
        Err(TaskCommandError::TaskNotFound { id: beta.id() })
    );
    assert_eq!(observable(access), observable_before_deleted_tag);

    // ---- record_batch_turn: Active admission plus idempotent no-op ----
    let revision_before_turn = access.revision();
    let turned = access
        .record_batch_turn(batch.value.id(), 1, true)
        .expect("active batch admits turn");
    assert_eq!(
        turned.revision(),
        Some(TaskRevision::new(revision_before_turn.get() + 1))
    );
    assert_eq!(turned.value.last_active_turn(), 1);
    assert_eq!(turned.value.silence_turns(), 0);

    let revision_after_first_turn = access.revision();
    let repeated_turn = access
        .record_batch_turn(batch.value.id(), 1, true)
        .expect("idempotent turn succeeds");
    assert_eq!(repeated_turn.revision(), None);
    assert!(repeated_turn.events.is_empty());
    assert_eq!(access.revision(), revision_after_first_turn);

    let silent_turn = access
        .record_batch_turn(batch.value.id(), 2, false)
        .expect("silence turn recorded");
    assert_eq!(
        silent_turn.revision(),
        Some(TaskRevision::new(revision_after_first_turn.get() + 1))
    );
    assert_eq!(silent_turn.value.silence_turns(), 1);

    // ---- Batch lifecycle: pause -> resume -> archive, where a duplicate
    // archive is the sole idempotent no-op terminal transition ----
    let revision_before_pause = access.revision();
    let paused = access
        .pause_batch(batch.value.id())
        .expect("pause succeeds");
    assert_eq!(
        paused.revision(),
        Some(TaskRevision::new(revision_before_pause.get() + 1))
    );
    assert_eq!(paused.value.status(), BatchStatus::Paused);

    // A Paused batch can no longer record turns; the rejection is atomic.
    let observable_before_paused_turn = observable(access);
    assert_eq!(
        access.record_batch_turn(batch.value.id(), 3, true),
        Err(TaskCommandError::BatchNotActive {
            id: batch.value.id(),
            status: BatchStatus::Paused,
        })
    );
    assert_eq!(observable(access), observable_before_paused_turn);

    let revision_before_resume = access.revision();
    let resumed = access
        .resume_batch(batch.value.id())
        .expect("resume succeeds");
    assert_eq!(
        resumed.revision(),
        Some(TaskRevision::new(revision_before_resume.get() + 1))
    );
    assert_eq!(resumed.value.status(), BatchStatus::Active);

    let revision_before_archive = access.revision();
    let archived = access
        .archive_batch(batch.value.id())
        .expect("archive succeeds");
    assert_eq!(
        archived.revision(),
        Some(TaskRevision::new(revision_before_archive.get() + 1))
    );
    assert_eq!(archived.value.status(), BatchStatus::Archived);

    // Archiving an already-archived batch is a true no-op: it keeps
    // succeeding and never reserves another revision, unlike every other
    // terminal transition attempted again below.
    let revision_after_archive = access.revision();
    let duplicate_archive = access
        .archive_batch(batch.value.id())
        .expect("duplicate archive succeeds");
    assert_eq!(duplicate_archive.revision(), None);
    assert!(duplicate_archive.events.is_empty());
    assert_eq!(duplicate_archive.value.status(), BatchStatus::Archived);
    assert_eq!(access.revision(), revision_after_archive);

    // An Archived batch can never record turns, pause, or resume again.
    let observable_before_archived_ops = observable(access);
    assert_eq!(
        access.record_batch_turn(batch.value.id(), 4, true),
        Err(TaskCommandError::BatchNotActive {
            id: batch.value.id(),
            status: BatchStatus::Archived,
        })
    );
    assert_eq!(
        access.pause_batch(batch.value.id()),
        Err(TaskCommandError::IllegalBatchTransition {
            id: batch.value.id(),
            from: BatchStatus::Archived,
            to: BatchStatus::Paused,
        })
    );
    assert_eq!(
        access.resume_batch(batch.value.id()),
        Err(TaskCommandError::IllegalBatchTransition {
            id: batch.value.id(),
            from: BatchStatus::Archived,
            to: BatchStatus::Active,
        })
    );
    assert_eq!(observable(access), observable_before_archived_ops);

    // ---- Revision overflow is atomic: no batch is admitted and the
    // authoritative revision never advances past `u64::MAX` ----
    assert_eq!(
        overflow
            .revision_exhausted
            .create_batch(batch_spec("overflow-revision"), 1),
        Err(TaskCommandError::RevisionExhausted)
    );
    assert_eq!(
        overflow.revision_exhausted.revision(),
        TaskRevision::new(u64::MAX)
    );
    assert!(overflow.revision_exhausted.list().is_empty());
    assert!(overflow.revision_exhausted.list_batches().is_empty());

    // ---- Batch ID overflow fails before any revision is reserved or any
    // batch becomes current ----
    assert_eq!(
        overflow
            .batch_id_exhausted
            .create_batch(batch_spec("overflow-batch-id"), 1),
        Err(TaskCommandError::BatchIdExhausted)
    );
    assert_eq!(overflow.batch_id_exhausted.revision(), TaskRevision::new(0));
    assert!(overflow.batch_id_exhausted.list_batches().is_empty());

    // ---- Task ID overflow fails atomically even though its owning Batch
    // was created successfully moments earlier ----
    let seeded_batch = overflow
        .task_id_exhausted
        .create_batch(batch_spec("seed"), 1)
        .expect("seed batch creation succeeds");
    let revision_before_task_overflow = overflow.task_id_exhausted.revision();
    assert_eq!(
        overflow
            .task_id_exhausted
            .create_task(task_spec("overflow-task-id"), 2),
        Err(TaskCommandError::TaskIdExhausted)
    );
    assert_eq!(
        overflow.task_id_exhausted.revision(),
        revision_before_task_overflow
    );
    assert!(overflow.task_id_exhausted.list().is_empty());
    assert_eq!(
        overflow.task_id_exhausted.list_batches(),
        vec![seeded_batch.value]
    );
}

#[test]
fn task_store_satisfies_task_access_contract() {
    use crate::business::TaskStoreState;
    use crate::{TaskId, TaskStore};

    let access = TaskStore::new();
    let revision_exhausted =
        TaskStore::from_state(TaskStoreState::empty().with_revision(TaskRevision::new(u64::MAX)));
    let batch_id_exhausted =
        TaskStore::from_state(TaskStoreState::empty().with_next_batch_id(BatchId::new(u64::MAX)));
    let task_id_exhausted =
        TaskStore::from_state(TaskStoreState::empty().with_next_task_id(TaskId::new(u64::MAX)));

    assert_task_access_contract(
        &access,
        TaskAccessOverflowFixtures {
            revision_exhausted: &revision_exhausted,
            batch_id_exhausted: &batch_id_exhausted,
            task_id_exhausted: &task_id_exhausted,
        },
    );
}

use super::*;
use crate::{
    BatchCreateSpec, TaskCommandError, TaskCreateSpec, TaskEvent, TaskPriority, TaskRevision,
};

fn batch_spec(name: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(name.into()).unwrap()
}

fn task_spec(name: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(name.into(), String::new(), None, TaskPriority::Normal).unwrap()
}

fn state_with_tasks(count: usize) -> (TaskStoreState, Vec<TaskId>) {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("批次"), 1).unwrap();
    let ids = (0..count)
        .map(|index| {
            state
                .create_task(task_spec(&format!("任务 {index}")), 2 + index as u64)
                .unwrap()
                .value
                .id()
        })
        .collect();
    (state, ids)
}

#[test]
fn empty_state_has_canonical_initial_values() {
    let state = TaskStoreState::empty();
    assert!(state.tasks().is_empty());
    assert!(state.batches().is_empty());
    assert_eq!(state.next_task_id(), TaskId::new(1));
    assert_eq!(state.next_batch_id(), BatchId::new(1));
    assert_eq!(state.current_batch(), None);
}

#[test]
fn add_dependency_updates_both_directions_and_is_idempotent() {
    let (mut state, ids) = state_with_tasks(2);
    let result = state.add_dependency(ids[0], ids[1], 10).unwrap();
    assert_eq!(state.tasks()[&ids[0]].blocked_by(), &[ids[1]]);
    assert_eq!(state.tasks()[&ids[1]].blocks(), &[ids[0]]);
    assert_eq!(
        result.events,
        vec![TaskEvent::TaskDependencyAdded {
            task_id: ids[0],
            blocked_by_id: ids[1],
        }]
    );

    let before = state.clone();
    let duplicate = state.add_dependency(ids[0], ids[1], 11).unwrap();
    assert_eq!(state, before);
    assert!(duplicate.events.is_empty());
}

#[test]
fn add_dependency_rejects_missing_self_cycle_and_indirect_cycle_atomically() {
    let (mut state, ids) = state_with_tasks(3);
    let before = state.clone();
    assert_eq!(
        state.add_dependency(TaskId::new(99), ids[0], 10),
        Err(TaskCommandError::TaskNotFound {
            id: TaskId::new(99)
        })
    );
    assert_eq!(state, before);
    assert_eq!(
        state.add_dependency(ids[0], TaskId::new(99), 10),
        Err(TaskCommandError::TaskNotFound {
            id: TaskId::new(99)
        })
    );
    assert_eq!(state, before);
    assert_eq!(
        state.add_dependency(ids[0], ids[0], 10),
        Err(TaskCommandError::DependencyCycle {
            task_id: ids[0],
            blocked_by_id: ids[0],
        })
    );
    assert_eq!(state, before);

    state.add_dependency(ids[0], ids[1], 10).unwrap();
    state.add_dependency(ids[1], ids[2], 11).unwrap();
    let before_cycle = state.clone();
    assert_eq!(
        state.add_dependency(ids[2], ids[0], 12),
        Err(TaskCommandError::DependencyCycle {
            task_id: ids[2],
            blocked_by_id: ids[0],
        })
    );
    assert_eq!(state, before_cycle);
}

#[test]
fn add_dependency_rejects_cross_batch_edge_atomically() {
    let (mut state, ids) = state_with_tasks(1);
    let first = ids[0];
    state.pause_batch(BatchId::new(1)).unwrap();
    state.create_batch(batch_spec("第二批"), 10).unwrap();
    let second = state
        .create_task(task_spec("第二批任务"), 11)
        .unwrap()
        .value
        .id();
    let before = state.clone();
    assert_eq!(
        state.add_dependency(second, first, 12),
        Err(TaskCommandError::CrossBatchDependency {
            task_id: second,
            blocked_by_id: first,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn add_dependency_rejects_deleted_endpoints_atomically() {
    let (mut state, ids) = state_with_tasks(2);
    state.delete(ids[1], 10).unwrap();
    let before = state.clone();
    assert_eq!(
        state.add_dependency(ids[0], ids[1], 11),
        Err(TaskCommandError::TaskNotFound { id: ids[1] })
    );
    assert_eq!(state, before);
    assert_eq!(
        state.add_dependency(ids[1], ids[0], 12),
        Err(TaskCommandError::TaskNotFound { id: ids[1] })
    );
    assert_eq!(state, before);
}

#[test]
fn remove_dependency_updates_both_directions_and_absent_edge_is_idempotent() {
    let (mut state, ids) = state_with_tasks(2);
    state.add_dependency(ids[0], ids[1], 10).unwrap();
    let result = state.remove_dependency(ids[0], ids[1], 11).unwrap();
    assert!(state.tasks()[&ids[0]].blocked_by().is_empty());
    assert!(state.tasks()[&ids[1]].blocks().is_empty());
    assert_eq!(
        result.events,
        vec![TaskEvent::TaskDependencyRemoved {
            task_id: ids[0],
            blocked_by_id: ids[1],
        }]
    );
    let before = state.clone();
    let absent = state.remove_dependency(ids[0], ids[1], 12).unwrap();
    assert_eq!(state, before);
    assert!(absent.events.is_empty());
}

#[test]
fn blocked_admission_is_atomic_until_every_dependency_completes() {
    let (mut state, ids) = state_with_tasks(3);
    state.add_dependency(ids[0], ids[1], 10).unwrap();
    state.add_dependency(ids[0], ids[2], 10).unwrap();
    let before = state.clone();
    assert_eq!(
        state.transition(ids[0], TaskStatus::InProgress, 20),
        Err(TaskCommandError::TaskBlocked {
            id: ids[0],
            blocked_by: vec![ids[1], ids[2]],
        })
    );
    assert_eq!(state, before);

    state.transition(ids[1], TaskStatus::Completed, 21).unwrap();
    assert!(matches!(
        state.transition(ids[0], TaskStatus::InProgress, 22),
        Err(TaskCommandError::TaskBlocked { .. })
    ));
    state.transition(ids[2], TaskStatus::Completed, 23).unwrap();
    state
        .transition(ids[0], TaskStatus::InProgress, 24)
        .unwrap();
    assert_eq!(state.tasks()[&ids[0]].status(), TaskStatus::InProgress);
}

#[test]
fn delete_cleans_all_incoming_and_outgoing_edges_atomically() {
    let (mut state, ids) = state_with_tasks(4);
    state.add_dependency(ids[0], ids[1], 10).unwrap();
    state.add_dependency(ids[1], ids[2], 11).unwrap();
    state.add_dependency(ids[3], ids[2], 12).unwrap();
    let result = state.delete(ids[1], 20).unwrap();
    assert_eq!(result.value.status(), TaskStatus::Deleted);
    assert!(state.tasks()[&ids[1]].blocked_by().is_empty());
    assert!(state.tasks()[&ids[1]].blocks().is_empty());
    assert!(state.tasks()[&ids[0]].blocked_by().is_empty());
    assert!(!state.tasks()[&ids[2]].blocks().contains(&ids[1]));
    assert_eq!(state.tasks()[&ids[3]].blocked_by(), &[ids[2]]);
    assert_eq!(
        result.events,
        vec![TaskEvent::TaskDeleted { task_id: ids[1] }]
    );
}

#[test]
fn delete_repeated_on_already_deleted_task_is_idempotent_no_op() {
    let (mut state, ids) = state_with_tasks(1);
    state.delete(ids[0], 20).unwrap();
    let before = state.clone();

    let duplicate = state.delete(ids[0], 30).unwrap();

    assert_eq!(
        state, before,
        "duplicate delete must not touch store state or revision"
    );
    assert_eq!(duplicate.revision(), None);
    assert!(duplicate.events.is_empty());
    assert_eq!(duplicate.value.status(), TaskStatus::Deleted);
}

#[test]
fn batch_commands_keep_current_batch_and_ids_consistent() {
    let mut state = TaskStoreState::empty();
    let first = state.create_batch(batch_spec("第一批"), 1).unwrap().value;
    assert_eq!(first.id(), BatchId::new(1));
    assert_eq!(state.current_batch(), Some(first.id()));
    assert_eq!(state.next_batch_id(), BatchId::new(2));

    let task_one = state.create_task(task_spec("任务一"), 2).unwrap().value;
    assert_eq!(task_one.id(), TaskId::new(1));
    assert_eq!(task_one.batch(), first.id());
    state.pause_batch(first.id()).unwrap();
    assert_eq!(state.current_batch(), None);
    let next_before_failure = state.next_task_id();
    assert_eq!(
        state.create_task(task_spec("无批次任务"), 3),
        Err(TaskCommandError::NoActiveBatch)
    );
    assert_eq!(state.next_task_id(), next_before_failure);

    let second = state.create_batch(batch_spec("第二批"), 4).unwrap().value;
    assert_eq!(second.id(), BatchId::new(2));
    let task_two = state.create_task(task_spec("任务二"), 5).unwrap().value;
    assert_eq!(task_two.id(), TaskId::new(2));
    assert_eq!(task_two.batch(), second.id());
    assert_eq!(state.next_task_id(), TaskId::new(3));
}

#[test]
fn resume_and_archive_enforce_single_active_batch_and_archived_terminal_state() {
    let mut state = TaskStoreState::empty();
    let first = state
        .create_batch(batch_spec("第一批"), 1)
        .unwrap()
        .value
        .id();
    state.pause_batch(first).unwrap();
    let second = state
        .create_batch(batch_spec("第二批"), 2)
        .unwrap()
        .value
        .id();
    let before = state.clone();
    assert_eq!(
        state.resume_batch(first),
        Err(TaskCommandError::ActiveBatchConflict {
            active: second,
            requested: first,
        })
    );
    assert_eq!(state, before);

    state.archive_batch(second).unwrap();
    assert_eq!(state.current_batch(), None);
    state.resume_batch(first).unwrap();
    assert_eq!(state.current_batch(), Some(first));
    state.archive_batch(first).unwrap();
    assert_eq!(state.current_batch(), None);
    assert_eq!(state.batches()[&first].status(), BatchStatus::Archived);
    state.archive_batch(first).unwrap();
    assert!(matches!(
        state.resume_batch(first),
        Err(TaskCommandError::IllegalBatchTransition { .. })
    ));
}

#[test]
fn deleting_task_and_archiving_batch_never_reuse_ids() {
    let (mut state, ids) = state_with_tasks(1);
    state.delete(ids[0], 10).unwrap();
    state.archive_batch(BatchId::new(1)).unwrap();
    let second_batch = state.create_batch(batch_spec("第二批"), 11).unwrap().value;
    let second_task = state.create_task(task_spec("第二任务"), 12).unwrap().value;
    assert_eq!(second_batch.id(), BatchId::new(2));
    assert_eq!(second_task.id(), TaskId::new(2));
}

#[test]
fn successful_mutations_commit_monotonic_revision_and_noop_mutations_stay_uncommitted() {
    let mut state = TaskStoreState::empty();
    assert_eq!(state.revision(), TaskRevision::new(0));

    let batch = state.create_batch(batch_spec("批次"), 1).unwrap();
    assert_eq!(batch.revision(), Some(TaskRevision::new(1)));
    assert_eq!(state.revision(), TaskRevision::new(1));

    let task = state.create_task(task_spec("任务"), 2).unwrap();
    assert_eq!(task.revision(), Some(TaskRevision::new(2)));
    assert_eq!(state.revision(), TaskRevision::new(2));

    let task_id = task.value.id();
    let noop = state
        .set_priority(task_id, TaskPriority::Normal, 3)
        .unwrap();
    assert_eq!(noop.revision(), None);
    assert_eq!(state.revision(), TaskRevision::new(2));
    assert!(noop.events.is_empty());

    let changed = state
        .set_priority(task_id, TaskPriority::Urgent, 4)
        .unwrap();
    assert_eq!(changed.revision(), Some(TaskRevision::new(3)));
    assert_eq!(state.revision(), TaskRevision::new(3));
    assert_eq!(
        changed.events,
        vec![TaskEvent::TaskPriorityChanged {
            task_id,
            from: TaskPriority::Normal,
            to: TaskPriority::Urgent,
        }]
    );
}

#[test]
fn add_and_remove_tag_return_task_and_stable_events_and_are_idempotent() {
    let (mut state, ids) = state_with_tasks(1);
    let id = ids[0];
    let base_revision = state.revision();

    let added = state.add_tag(id, "backend".into(), 10).unwrap();
    assert_eq!(added.value.tags(), &["backend".to_string()]);
    assert_eq!(
        added.events,
        vec![TaskEvent::TaskTagAdded {
            task_id: id,
            tag: "backend".into(),
        }]
    );
    assert_eq!(
        added.revision(),
        Some(TaskRevision::new(base_revision.get() + 1))
    );
    assert_eq!(state.revision(), TaskRevision::new(base_revision.get() + 1));

    let duplicate = state.add_tag(id, "backend".into(), 11).unwrap();
    assert!(duplicate.events.is_empty());
    assert_eq!(duplicate.revision(), None);
    assert_eq!(state.revision(), TaskRevision::new(base_revision.get() + 1));

    let removed = state.remove_tag(id, "backend", 12).unwrap();
    assert!(removed.value.tags().is_empty());
    assert_eq!(
        removed.events,
        vec![TaskEvent::TaskTagRemoved {
            task_id: id,
            tag: "backend".into(),
        }]
    );
    assert_eq!(state.revision(), TaskRevision::new(base_revision.get() + 2));

    let absent = state.remove_tag(id, "missing", 13).unwrap();
    assert!(absent.events.is_empty());
    assert_eq!(absent.revision(), None);
    assert_eq!(state.revision(), TaskRevision::new(base_revision.get() + 2));
}

#[test]
fn dependency_commands_return_the_primary_task_and_commit_revision() {
    let (mut state, ids) = state_with_tasks(2);
    let base_revision = state.revision();

    let added = state.add_dependency(ids[0], ids[1], 10).unwrap();
    assert_eq!(added.value.id(), ids[0]);
    assert_eq!(added.value.blocked_by(), &[ids[1]]);
    assert_eq!(
        added.revision(),
        Some(TaskRevision::new(base_revision.get() + 1))
    );

    let duplicate = state.add_dependency(ids[0], ids[1], 11).unwrap();
    assert_eq!(duplicate.value.id(), ids[0]);
    assert_eq!(duplicate.revision(), None);

    let removed = state.remove_dependency(ids[0], ids[1], 12).unwrap();
    assert_eq!(removed.value.id(), ids[0]);
    assert!(removed.value.blocked_by().is_empty());
    assert_eq!(
        removed.revision(),
        Some(TaskRevision::new(base_revision.get() + 2))
    );

    let absent = state.remove_dependency(ids[0], ids[1], 13).unwrap();
    assert_eq!(absent.value.id(), ids[0]);
    assert_eq!(absent.revision(), None);
}

#[test]
fn batch_lifecycle_commands_return_batch_command_results_with_revision() {
    let mut state = TaskStoreState::empty();
    let created = state.create_batch(batch_spec("批次"), 1).unwrap();
    let id = created.value.id();
    assert_eq!(created.revision(), Some(TaskRevision::new(1)));

    let paused = state.pause_batch(id).unwrap();
    assert_eq!(paused.value.status(), BatchStatus::Paused);
    assert_eq!(paused.revision(), Some(TaskRevision::new(2)));

    let resumed = state.resume_batch(id).unwrap();
    assert_eq!(resumed.value.status(), BatchStatus::Active);
    assert_eq!(resumed.revision(), Some(TaskRevision::new(3)));

    let archived = state.archive_batch(id).unwrap();
    assert_eq!(archived.value.status(), BatchStatus::Archived);
    assert_eq!(archived.revision(), Some(TaskRevision::new(4)));
}

#[test]
fn record_batch_turn_tracks_last_active_turn_and_silence_and_commits_revision() {
    let mut state = TaskStoreState::empty();
    let id = state
        .create_batch(batch_spec("批次"), 1)
        .unwrap()
        .value
        .id();
    let base_revision = state.revision();

    let silent = state.record_batch_turn(id, 5, false).unwrap();
    assert_eq!(silent.value.silence_turns(), 1);
    assert_eq!(silent.value.last_active_turn(), 0);
    assert_eq!(
        silent.revision(),
        Some(TaskRevision::new(base_revision.get() + 1))
    );

    let silent_again = state.record_batch_turn(id, 6, false).unwrap();
    assert_eq!(silent_again.value.silence_turns(), 2);

    let active = state.record_batch_turn(id, 7, true).unwrap();
    assert_eq!(active.value.silence_turns(), 0);
    assert_eq!(active.value.last_active_turn(), 7);

    let before = state.clone();
    assert_eq!(
        state.record_batch_turn(BatchId::new(99), 8, true),
        Err(TaskCommandError::BatchNotFound {
            id: BatchId::new(99)
        })
    );
    assert_eq!(state, before);
}

#[test]
fn task_and_batch_id_exhaustion_is_rejected_and_state_is_unchanged() {
    let mut state = TaskStoreState::empty().with_next_batch_id(BatchId::new(u64::MAX));
    let before = state.clone();
    assert_eq!(
        state.create_batch(batch_spec("批次"), 1),
        Err(TaskCommandError::BatchIdExhausted)
    );
    assert_eq!(state, before);

    let mut state = TaskStoreState::empty().with_next_task_id(TaskId::new(u64::MAX));
    state.create_batch(batch_spec("批次"), 1).unwrap();
    let before = state.clone();
    assert_eq!(
        state.create_task(task_spec("任务"), 2),
        Err(TaskCommandError::TaskIdExhausted)
    );
    assert_eq!(state, before);
}

#[test]
fn revision_exhaustion_is_rejected_and_state_is_unchanged() {
    let mut state = TaskStoreState::empty().with_revision(TaskRevision::new(u64::MAX));
    let before = state.clone();
    assert_eq!(
        state.create_batch(batch_spec("批次"), 1),
        Err(TaskCommandError::RevisionExhausted)
    );
    assert_eq!(state, before);
}

#[test]
fn archive_batch_is_idempotent_noop_and_tolerates_revision_exhaustion() {
    let mut state = TaskStoreState::empty();
    let id = state
        .create_batch(batch_spec("批次"), 1)
        .unwrap()
        .value
        .id();

    // First archive is a real, revision-committing transition.
    let archived = state.archive_batch(id).unwrap();
    assert_eq!(archived.value.status(), BatchStatus::Archived);
    assert!(archived.revision().is_some());
    let revision_after_first_archive = state.revision();

    // Re-archiving an already-archived batch must be a true no-op: no event,
    // no revision, and the stored batch/state must stay byte-for-byte equal.
    let before = state.clone();
    let duplicate = state.archive_batch(id).unwrap();
    assert!(duplicate.events.is_empty());
    assert_eq!(duplicate.revision(), None);
    assert_eq!(duplicate.value.status(), BatchStatus::Archived);
    assert_eq!(state, before);
    assert_eq!(state.revision(), revision_after_first_archive);

    // Even when the store's revision counter is already exhausted, a repeat
    // archive call must still succeed as a no-op instead of surfacing
    // `RevisionExhausted`, because it never needs to reserve a revision.
    let mut saturated = state.with_revision(TaskRevision::new(u64::MAX));
    let before = saturated.clone();
    let duplicate_at_max = saturated.archive_batch(id).unwrap();
    assert!(duplicate_at_max.events.is_empty());
    assert_eq!(duplicate_at_max.revision(), None);
    assert_eq!(saturated, before);
    assert_eq!(saturated.revision(), TaskRevision::new(u64::MAX));
}

#[test]
fn record_batch_turn_rejects_non_active_batch_and_leaves_state_unchanged() {
    let mut state = TaskStoreState::empty();
    let id = state
        .create_batch(batch_spec("批次"), 1)
        .unwrap()
        .value
        .id();

    state.pause_batch(id).unwrap();
    let before = state.clone();
    assert_eq!(
        state.record_batch_turn(id, 5, true),
        Err(TaskCommandError::BatchNotActive {
            id,
            status: BatchStatus::Paused,
        })
    );
    assert_eq!(state, before);

    state.resume_batch(id).unwrap();
    state.archive_batch(id).unwrap();
    let before = state.clone();
    assert_eq!(
        state.record_batch_turn(id, 5, true),
        Err(TaskCommandError::BatchNotActive {
            id,
            status: BatchStatus::Archived,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn record_batch_turn_is_noop_when_no_effective_change() {
    let id = BatchId::new(1);
    let mut state = TaskStoreState::empty()
        .with_next_batch_id(BatchId::new(2))
        .with_batch(Batch::with_status(id, BatchStatus::Active, 0));
    let base_revision = state.revision();

    // Already at last_active_turn == 0 with silence_turns == 0: recording the
    // same active turn again changes nothing.
    let noop_active = state.record_batch_turn(id, 0, true).unwrap();
    assert!(noop_active.events.is_empty());
    assert_eq!(noop_active.revision(), None);
    assert_eq!(state.revision(), base_revision);
    assert_eq!(state.batches()[&id].last_active_turn(), 0);
    assert_eq!(state.batches()[&id].silence_turns(), 0);

    let saturated_id = BatchId::new(2);
    let mut saturated_state = TaskStoreState::empty()
        .with_next_batch_id(BatchId::new(3))
        .with_batch(Batch::with_status(
            saturated_id,
            BatchStatus::Active,
            u64::MAX,
        ));
    let base_revision = saturated_state.revision();

    // silence_turns already saturated at u64::MAX: another silent turn cannot
    // change the value (saturating_add would no-op anyway), so no revision.
    let noop_silent = saturated_state
        .record_batch_turn(saturated_id, 99, false)
        .unwrap();
    assert!(noop_silent.events.is_empty());
    assert_eq!(noop_silent.revision(), None);
    assert_eq!(saturated_state.revision(), base_revision);
    assert_eq!(
        saturated_state.batches()[&saturated_id].silence_turns(),
        u64::MAX
    );
}

#[test]
fn clear_is_one_atomic_revision_and_empty_clear_is_noop() {
    let (mut state, _) = state_with_tasks(2);
    let before = state.revision();
    let cleared = state.clear().expect("clear succeeds");
    assert_eq!(
        cleared.revision(),
        Some(TaskRevision::new(before.get() + 1))
    );
    assert_eq!(
        cleared.events,
        vec![TaskEvent::TaskStoreCleared {
            task_count: 2,
            batch_count: 1,
        }]
    );
    assert!(state.list().is_empty());
    assert!(state.list_batches().is_empty());
    assert_eq!(state.current_batch(), None);
    assert_eq!(state.next_task_id(), TaskId::new(1));
    assert_eq!(state.next_batch_id(), BatchId::new(1));

    let revision = state.revision();
    let noop = state.clear().expect("empty clear succeeds");
    assert_eq!(noop.revision(), None);
    assert!(noop.events.is_empty());
    assert_eq!(state.revision(), revision);
}

#[test]
fn clear_at_revision_exhaustion_is_atomic() {
    let (state, _) = state_with_tasks(1);
    let mut state = state.with_revision(TaskRevision::new(u64::MAX));
    let before = state.clone();
    assert_eq!(state.clear(), Err(TaskCommandError::RevisionExhausted));
    assert_eq!(state, before);
}

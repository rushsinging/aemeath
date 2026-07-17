use super::*;
use crate::{BatchCreateSpec, TaskCommandError, TaskCreateSpec, TaskEvent, TaskPriority};

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

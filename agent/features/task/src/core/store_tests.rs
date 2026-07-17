use std::sync::Arc;
use std::thread;

use super::{TaskAccess, TaskStore};
use crate::business::{
    BatchCreateSpec, TaskCommandError, TaskCreateSpec, TaskEvent, TaskId, TaskPriority,
    TaskRevision, TaskStatus, TaskStoreState,
};

fn batch_spec(name: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(name.into()).unwrap()
}

fn task_spec(name: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(name.into(), String::new(), None, TaskPriority::Normal).unwrap()
}

// ---- 结构性质：同一同步锁槽 ----

#[test]
fn store_is_send_and_sync_for_shared_use_without_holding_guard_across_await() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TaskStore>();
}

#[test]
fn new_store_starts_at_empty_revision_zero() {
    let store = TaskStore::new();
    assert_eq!(store.revision(), TaskRevision::new(0));
    assert!(store.list().is_empty());
    assert!(store.list_batches().is_empty());
    assert_eq!(store.reminder_snapshot().current_batch, None);
}

#[test]
fn default_store_matches_new() {
    let store = TaskStore::default();
    assert_eq!(store.revision(), TaskRevision::new(0));
    assert_eq!(store.reminder_snapshot().current_batch, None);
}

#[test]
fn poisoned_store_fails_closed_instead_of_exposing_state() {
    let store = Arc::new(TaskStore::new());
    let poisoning_store = Arc::clone(&store);

    let poison = thread::spawn(move || {
        let _guard = poisoning_store.state.lock().unwrap();
        panic!("simulate a panic during a transaction");
    });
    assert!(poison.join().is_err());

    let access = std::panic::catch_unwind(|| store.revision());
    assert!(access.is_err(), "a poisoned store must stop serving state");
}

// ---- 真实改变一次 revision；结果 revision 与 value/events 同一提交 ----

#[test]
fn real_mutation_advances_revision_exactly_once_and_commits_with_value_and_events() {
    let store = TaskStore::new();

    let batch = store.create_batch(batch_spec("批次"), 1).unwrap();
    assert_eq!(batch.revision(), Some(TaskRevision::new(1)));
    assert_eq!(store.revision(), TaskRevision::new(1));

    let created = store.create_task(task_spec("任务"), 2).unwrap();
    assert_eq!(created.revision(), Some(TaskRevision::new(2)));
    assert_eq!(store.revision(), TaskRevision::new(2));
    assert_eq!(
        created.events,
        vec![TaskEvent::TaskCreated {
            task_id: created.value.id()
        }]
    );
}

// ---- failure 不变 ----

#[test]
fn failure_command_leaves_revision_and_state_untouched() {
    let store = TaskStore::new();
    let before_revision = store.revision();

    let err = store.create_task(task_spec("任务"), 1).unwrap_err();

    assert_eq!(err, TaskCommandError::NoActiveBatch);
    assert_eq!(store.revision(), before_revision);
    assert!(store.list().is_empty());
}

// ---- query 不变 ----

#[test]
fn query_methods_never_advance_revision() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("批次"), 1).unwrap();
    let created = store.create_task(task_spec("任务"), 2).unwrap();
    let id = created.value.id();
    let before = store.revision();

    let _ = store.get(id);
    let _ = store.list();
    let _ = store.list_batches();
    let _ = store.stats();
    let _ = store.reminder_snapshot();
    let _ = store.lifecycle_snapshot(10);
    let _ = store.is_blocked(id).unwrap();
    let _ = store.would_create_cycle(id, id);

    assert_eq!(store.revision(), before);
}

// ---- no-op 不变 ----

#[test]
fn idempotent_no_op_mutation_does_not_advance_revision() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("批次"), 1).unwrap();
    let created = store.create_task(task_spec("任务"), 2).unwrap();
    let id = created.value.id();
    let after_create = store.revision();

    let noop = store.set_priority(id, created.value.priority(), 3).unwrap();

    assert!(noop.events.is_empty());
    assert_eq!(noop.revision(), None);
    assert_eq!(store.revision(), after_create);
}

// ---- overflow 原子失败 ----

#[test]
fn revision_overflow_fails_atomically_without_partial_mutation() {
    let store =
        TaskStore::from_state(TaskStoreState::empty().with_revision(TaskRevision::new(u64::MAX)));
    let before = store.state_snapshot();

    let err = store.create_batch(batch_spec("批次"), 1).unwrap_err();

    assert_eq!(err, TaskCommandError::RevisionExhausted);
    assert_eq!(store.state_snapshot(), before);
}

#[test]
fn task_id_overflow_fails_atomically_without_partial_mutation() {
    let store =
        TaskStore::from_state(TaskStoreState::empty().with_next_task_id(TaskId::new(u64::MAX)));
    store.create_batch(batch_spec("批次"), 1).unwrap();
    let before = store.state_snapshot();

    let err = store.create_task(task_spec("任务"), 2).unwrap_err();

    assert_eq!(err, TaskCommandError::TaskIdExhausted);
    assert_eq!(store.state_snapshot(), before);
}

// ---- 全命令面委托 aggregate ----

#[test]
fn dependency_and_tag_commands_delegate_and_commit_revision_in_order() {
    let store = TaskStore::new();
    store.create_batch(batch_spec("批次"), 0).unwrap();
    let a = store.create_task(task_spec("A"), 1).unwrap().value;
    let b = store.create_task(task_spec("B"), 2).unwrap().value;
    assert_eq!(store.revision(), TaskRevision::new(3));

    assert!(!store.would_create_cycle(a.id(), b.id()));

    let dep = store.add_dependency(a.id(), b.id(), 3).unwrap();
    assert_eq!(dep.revision(), Some(TaskRevision::new(4)));
    assert!(store.is_blocked(a.id()).unwrap());

    let tagged = store.add_tag(a.id(), "urgent".into(), 4).unwrap();
    assert_eq!(tagged.revision(), Some(TaskRevision::new(5)));
    assert!(tagged.value.tags().contains(&"urgent".to_string()));

    let untagged = store.remove_tag(a.id(), "urgent", 5).unwrap();
    assert_eq!(untagged.revision(), Some(TaskRevision::new(6)));
    assert!(untagged.value.tags().is_empty());

    let undep = store.remove_dependency(a.id(), b.id(), 6).unwrap();
    assert_eq!(undep.revision(), Some(TaskRevision::new(7)));
    assert!(!store.is_blocked(a.id()).unwrap());

    assert_eq!(store.revision(), TaskRevision::new(7));
}

#[test]
fn lifecycle_transition_and_batch_commands_delegate_and_commit_revision_in_order() {
    let store = TaskStore::new();
    let batch = store.create_batch(batch_spec("批次"), 0).unwrap().value;
    let a = store.create_task(task_spec("A"), 1).unwrap().value;
    assert_eq!(store.revision(), TaskRevision::new(2));

    let started = store.transition(a.id(), TaskStatus::InProgress, 2).unwrap();
    assert_eq!(started.revision(), Some(TaskRevision::new(3)));

    let completed = store.transition(a.id(), TaskStatus::Completed, 3).unwrap();
    assert_eq!(completed.revision(), Some(TaskRevision::new(4)));

    let priority = store.set_priority(a.id(), TaskPriority::High, 4).unwrap();
    assert_eq!(priority.revision(), Some(TaskRevision::new(5)));

    let turned = store.record_batch_turn(batch.id(), 1, true).unwrap();
    assert_eq!(turned.revision(), Some(TaskRevision::new(6)));

    let deleted = store.delete(a.id(), 5).unwrap();
    assert_eq!(deleted.revision(), Some(TaskRevision::new(7)));
    assert_eq!(store.stats().deleted, 1);

    let paused = store.pause_batch(batch.id()).unwrap();
    assert_eq!(paused.revision(), Some(TaskRevision::new(8)));
    assert_eq!(store.reminder_snapshot().current_batch, None);

    let resumed = store.resume_batch(batch.id()).unwrap();
    assert_eq!(resumed.revision(), Some(TaskRevision::new(9)));
    assert_eq!(store.reminder_snapshot().current_batch, Some(batch.id()));

    let archived = store.archive_batch(batch.id()).unwrap();
    assert_eq!(archived.revision(), Some(TaskRevision::new(10)));
    assert_eq!(store.reminder_snapshot().current_batch, None);

    assert_eq!(store.revision(), TaskRevision::new(10));

    let reminder = store.reminder_snapshot();
    assert_eq!(reminder.current_batch, None);
    let snapshot = store.lifecycle_snapshot(1_000);
    assert_eq!(snapshot.current_batch, None);
}

// ---- 并发下同一锁槽保证 revision 唯一且不丢更新 ----

#[test]
fn concurrent_task_creation_produces_unique_sequential_revisions() {
    let store = Arc::new(TaskStore::new());
    store.create_batch(batch_spec("批次"), 0).unwrap();

    let handles: Vec<_> = (0..8)
        .map(|index| {
            let store = Arc::clone(&store);
            thread::spawn(move || {
                store
                    .create_task(task_spec(&format!("任务 {index}")), index as u64)
                    .unwrap()
            })
        })
        .collect();

    let mut revisions: Vec<u64> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().revision().unwrap().get())
        .collect();
    revisions.sort_unstable();

    assert_eq!(revisions, (2..=9).collect::<Vec<_>>());
    assert_eq!(store.list().len(), 8);
    assert_eq!(store.revision(), TaskRevision::new(9));
}

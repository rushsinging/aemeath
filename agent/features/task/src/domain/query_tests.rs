use super::*;

fn batch_spec(name: &str) -> BatchCreateSpec {
    BatchCreateSpec::try_new(name.into()).unwrap()
}

fn task_spec(name: &str, priority: TaskPriority) -> TaskCreateSpec {
    TaskCreateSpec::try_new(name.into(), String::new(), None, priority).unwrap()
}

#[test]
fn get_and_collections_are_owned_and_sorted_by_typed_id() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("first"), 1).unwrap();
    let one = state
        .create_task(task_spec("one", TaskPriority::Normal), 2)
        .unwrap()
        .value;
    let two = state
        .create_task(task_spec("two", TaskPriority::High), 3)
        .unwrap()
        .value;
    state.pause_batch(BatchId::new(1)).unwrap();
    state.create_batch(batch_spec("second"), 4).unwrap();
    let three = state
        .create_task(task_spec("three", TaskPriority::Low), 5)
        .unwrap()
        .value;

    assert_eq!(state.get(two.id()), Some(two));
    assert_eq!(state.get(TaskId::new(99)), None);
    assert_eq!(
        state.list().iter().map(Task::id).collect::<Vec<_>>(),
        vec![one.id(), TaskId::new(2), three.id()]
    );
    assert_eq!(
        state
            .list_batches()
            .iter()
            .map(Batch::id)
            .collect::<Vec<_>>(),
        vec![BatchId::new(1), BatchId::new(2)]
    );
}

#[test]
fn stats_and_reminder_are_deterministic_pure_values() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("batch"), 1).unwrap();
    let pending = state
        .create_task(task_spec("pending", TaskPriority::Urgent), 2)
        .unwrap()
        .value
        .id();
    let blocker = state
        .create_task(task_spec("blocker", TaskPriority::Low), 3)
        .unwrap()
        .value
        .id();
    let completed = state
        .create_task(task_spec("completed", TaskPriority::Urgent), 4)
        .unwrap()
        .value
        .id();
    let deleted = state
        .create_task(task_spec("deleted", TaskPriority::High), 5)
        .unwrap()
        .value
        .id();
    state.add_dependency(pending, blocker, 6).unwrap();
    state
        .transition(completed, TaskStatus::Completed, 7)
        .unwrap();
    state.delete(deleted, 8).unwrap();
    assert_eq!(state.get(deleted).unwrap().status(), TaskStatus::Deleted);
    assert!(!state.list().iter().any(|task| task.id() == deleted));

    let stats = state.stats();
    assert_eq!(
        (stats.total, stats.pending, stats.completed, stats.deleted),
        (4, 2, 1, 1)
    );
    assert_eq!(stats.by_priority.low, 1);
    assert_eq!(stats.by_priority.urgent, 2);
    assert_eq!(stats.by_priority.high, 0); // deleted tasks are excluded from priority totals

    let reminder = state.reminder_snapshot();
    assert_eq!(reminder.current_batch, Some(BatchId::new(1)));
    assert_eq!(
        reminder
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![pending, blocker, completed]
    );
    assert!(reminder.items[0].blocked);
    assert_eq!(reminder.items[0].subject, "pending");
}

#[test]
fn lifecycle_queries_reuse_authoritative_state() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("batch"), 1).unwrap();
    let first = state
        .create_task(task_spec("first", TaskPriority::Normal), 2)
        .unwrap()
        .value
        .id();
    let second = state
        .create_task(task_spec("second", TaskPriority::Normal), 3)
        .unwrap()
        .value
        .id();
    state.add_dependency(first, second, 4).unwrap();

    assert!(state.would_create_cycle(second, first));
    assert!(!state.would_create_cycle(first, second));
    state.transition(second, TaskStatus::Completed, 5).unwrap();
    state.transition(first, TaskStatus::Completed, 6).unwrap();
    state.record_batch_turn(BatchId::new(1), 9, false).unwrap();
    let lifecycle = state.lifecycle_snapshot(1);
    assert_eq!(lifecycle.all_completed, Some(BatchId::new(1)));
    assert_eq!(lifecycle.interrupted, None);
    assert!(lifecycle.stale_batches.is_empty());
}

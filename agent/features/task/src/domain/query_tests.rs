use super::*;

fn batch_spec(name: &str) -> BatchCreateSpecData {
    BatchCreateSpecData::try_new(name.into()).unwrap()
}

fn task_spec(name: &str, priority: TaskPriorityData) -> TaskCreateSpecData {
    TaskCreateSpecData::try_new(name.into(), String::new(), None, priority).unwrap()
}

#[test]
fn get_and_collections_are_owned_and_sorted_by_typed_id() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("first"), 1).unwrap();
    let one = state
        .create_task(task_spec("one", TaskPriorityData::Normal), 2)
        .unwrap()
        .value;
    let two = state
        .create_task(task_spec("two", TaskPriorityData::High), 3)
        .unwrap()
        .value;
    state.pause_batch(BatchIdData::new(1)).unwrap();
    state.create_batch(batch_spec("second"), 4).unwrap();
    let three = state
        .create_task(task_spec("three", TaskPriorityData::Low), 5)
        .unwrap()
        .value;

    assert_eq!(state.get(two.id()), Some(two));
    assert_eq!(state.get(TaskIdData::new(99)), None);
    assert_eq!(
        state.list().iter().map(TaskData::id).collect::<Vec<_>>(),
        vec![one.id(), TaskIdData::new(2), three.id()]
    );
    assert_eq!(
        state
            .list_batches()
            .iter()
            .map(BatchData::id)
            .collect::<Vec<_>>(),
        vec![BatchIdData::new(1), BatchIdData::new(2)]
    );
}

#[test]
fn batch_snapshots_scope_stats_and_tasks_to_each_batch() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("first"), 1).unwrap();
    let first_pending = state
        .create_task(task_spec("first pending", TaskPriorityData::High), 2)
        .unwrap()
        .value;
    let first_completed = state
        .create_task(task_spec("first completed", TaskPriorityData::Normal), 3)
        .unwrap()
        .value;
    let first_deleted = state
        .create_task(task_spec("first deleted", TaskPriorityData::Urgent), 4)
        .unwrap()
        .value;
    state
        .transition(first_completed.id(), TaskStatusData::Completed, 5)
        .unwrap();
    state.delete(first_deleted.id(), 6).unwrap();
    state.pause_batch(BatchIdData::new(1)).unwrap();

    state.create_batch(batch_spec("second"), 7).unwrap();
    let second = state
        .create_task(task_spec("second", TaskPriorityData::Low), 8)
        .unwrap()
        .value;
    state
        .transition(second.id(), TaskStatusData::InProgress, 9)
        .unwrap();

    let first = state.batch_snapshot(BatchIdData::new(1)).unwrap();
    assert_eq!(first.batch().summary(), Some("first"));
    assert_eq!(first.stats().total, 2);
    assert_eq!(first.stats().pending, 1);
    assert_eq!(first.stats().completed, 1);
    assert_eq!(
        first.tasks().iter().map(TaskData::id).collect::<Vec<_>>(),
        vec![first_pending.id(), first_completed.id()]
    );

    let second_snapshot = state.batch_snapshot(BatchIdData::new(2)).unwrap();
    assert_eq!(second_snapshot.stats().total, 1);
    assert_eq!(second_snapshot.stats().in_progress, 1);
    assert_eq!(second_snapshot.tasks()[0].id(), second.id());
    assert_eq!(state.batch_snapshot(BatchIdData::new(99)), None);
    assert_eq!(
        state
            .list_batch_snapshots()
            .iter()
            .map(|snapshot| snapshot.batch().id())
            .collect::<Vec<_>>(),
        vec![BatchIdData::new(1), BatchIdData::new(2)]
    );
}

#[test]
fn progress_snapshot_sorts_limits_and_classifies_current_batch_tasks() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("batch"), 1).unwrap();

    let completed_oldest = state
        .create_task(task_spec("completed oldest", TaskPriorityData::Normal), 2)
        .unwrap()
        .value
        .id();
    let completed_middle = state
        .create_task(task_spec("completed middle", TaskPriorityData::Normal), 3)
        .unwrap()
        .value
        .id();
    let completed_latest = state
        .create_task(task_spec("completed latest", TaskPriorityData::Normal), 4)
        .unwrap()
        .value
        .id();
    let doing = (0..3)
        .map(|index| {
            state
                .create_task(
                    task_spec(&format!("doing {index}"), TaskPriorityData::Normal),
                    5 + index,
                )
                .unwrap()
                .value
                .id()
        })
        .collect::<Vec<_>>();
    let ready = (0..3)
        .map(|index| {
            state
                .create_task(
                    task_spec(&format!("ready {index}"), TaskPriorityData::Normal),
                    8 + index,
                )
                .unwrap()
                .value
                .id()
        })
        .collect::<Vec<_>>();
    let blocked = (0..2)
        .map(|index| {
            state
                .create_task(
                    task_spec(&format!("blocked {index}"), TaskPriorityData::Normal),
                    11 + index,
                )
                .unwrap()
                .value
                .id()
        })
        .collect::<Vec<_>>();

    state
        .transition(completed_oldest, TaskStatusData::Completed, 20)
        .unwrap();
    state
        .transition(completed_middle, TaskStatusData::Completed, 21)
        .unwrap();
    state
        .transition(completed_latest, TaskStatusData::Completed, 22)
        .unwrap();
    for (offset, id) in doing.iter().enumerate() {
        state
            .transition(*id, TaskStatusData::InProgress, 30 + offset as u64)
            .unwrap();
    }
    state.add_dependency(blocked[0], doing[0], 40).unwrap();
    state.add_dependency(blocked[1], doing[1], 41).unwrap();

    let snapshot = state
        .progress_snapshot(BatchIdData::new(1), completed_latest, false, false)
        .unwrap();

    assert_eq!(
        snapshot
            .recently_completed
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![completed_latest, completed_middle]
    );
    assert_eq!(
        snapshot
            .in_progress
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        doing
    );
    assert_eq!(
        snapshot
            .ready
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        ready[..2] // allow unsafe_text_op: Vec slice
    );
    assert_eq!(snapshot.ready_omitted, 1);
    assert_eq!(snapshot.blocked_count, 2);
    assert_eq!(snapshot.updated.id, completed_latest);
}

#[test]
fn stats_are_deterministic_pure_values() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("batch"), 1).unwrap();
    let pending = state
        .create_task(task_spec("pending", TaskPriorityData::Urgent), 2)
        .unwrap()
        .value
        .id();
    let blocker = state
        .create_task(task_spec("blocker", TaskPriorityData::Low), 3)
        .unwrap()
        .value
        .id();
    let completed = state
        .create_task(task_spec("completed", TaskPriorityData::Urgent), 4)
        .unwrap()
        .value
        .id();
    let deleted = state
        .create_task(task_spec("deleted", TaskPriorityData::High), 5)
        .unwrap()
        .value
        .id();
    state.add_dependency(pending, blocker, 6).unwrap();
    state
        .transition(completed, TaskStatusData::Completed, 7)
        .unwrap();
    state.delete(deleted, 8).unwrap();
    assert_eq!(
        state.get(deleted).unwrap().status(),
        TaskStatusData::Deleted
    );
    assert!(!state.list().iter().any(|task| task.id() == deleted));

    let stats = state.stats();
    assert_eq!(
        (stats.total, stats.pending, stats.completed, stats.deleted),
        (4, 2, 1, 1)
    );
    assert_eq!(stats.by_priority.low, 1);
    assert_eq!(stats.by_priority.urgent, 2);
    assert_eq!(stats.by_priority.high, 0); // deleted tasks are excluded from priority totals
}

#[test]
fn lifecycle_queries_reuse_authoritative_state() {
    let mut state = TaskStoreState::empty();
    state.create_batch(batch_spec("batch"), 1).unwrap();
    let first = state
        .create_task(task_spec("first", TaskPriorityData::Normal), 2)
        .unwrap()
        .value
        .id();
    let second = state
        .create_task(task_spec("second", TaskPriorityData::Normal), 3)
        .unwrap()
        .value
        .id();
    state.add_dependency(first, second, 4).unwrap();

    assert!(state.would_create_cycle(second, first));
    assert!(!state.would_create_cycle(first, second));
    state
        .transition(second, TaskStatusData::Completed, 5)
        .unwrap();
    state
        .transition(first, TaskStatusData::Completed, 6)
        .unwrap();
    state
        .record_batch_turn(BatchIdData::new(1), 9, false)
        .unwrap();
    let lifecycle = state.lifecycle_snapshot(1);
    assert_eq!(lifecycle.all_completed, Some(BatchIdData::new(1)));
    assert_eq!(lifecycle.interrupted, None);
    assert!(lifecycle.stale_batches.is_empty());
}

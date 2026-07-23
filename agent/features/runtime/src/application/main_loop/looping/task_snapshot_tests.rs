use super::*;
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskStatus};

fn task_spec(subject: &str) -> TaskCreateSpec {
    TaskCreateSpec::try_new(
        subject.to_owned(),
        String::new(),
        None,
        TaskPriority::Normal,
    )
    .unwrap()
}

fn access_with_active_batch() -> task::TaskStore {
    let store = task::TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".into()).unwrap(), 1)
        .unwrap();
    store
}

#[test]
fn task_snapshot_renders_task_owned_projection_without_owner() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    let created = access.create_task(task_spec("实现适配器"), 2).unwrap();

    let snapshot = build_task_snapshot(access);

    assert_eq!(snapshot.lines[0], "━━ Tasks: 0/1 ━━");
    assert!(snapshot.lines[1].contains(&format!("#{}", created.value.id())));
    assert!(snapshot.lines[1].contains("实现适配器"));
    assert!(!snapshot.lines[1].contains('@'));
}

#[test]
fn task_snapshot_empty_without_active_batch() {
    let store = task::TaskStore::new();
    let access: &dyn TaskAccess = &store;
    assert!(build_task_snapshot(access).lines.is_empty());
}

#[test]
fn task_status_lines_orders_statuses_and_formats_dependencies() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    let completed = access.create_task(task_spec("completed"), 2).unwrap().value;
    let in_progress = access.create_task(task_spec("working"), 3).unwrap().value;
    let pending = access.create_task(task_spec("blocked"), 4).unwrap().value;
    access
        .transition(completed.id(), TaskStatus::Completed, 5)
        .unwrap();
    access
        .transition(in_progress.id(), TaskStatus::InProgress, 6)
        .unwrap();
    access
        .add_dependency(pending.id(), completed.id(), 7)
        .unwrap();

    let lines = task_status_lines(&access.list(), 7);

    assert_eq!(lines[0], "━━ Tasks: 1/3 ━━");
    assert!(lines[1].contains("✓ #1 completed"));
    assert!(lines[2].contains("■ #2 working"));
    assert!(lines[3].contains("□ #3 blocked (blocked by #1)"));
}

#[test]
fn task_status_lines_limits_visible_tasks_and_reports_hidden_count() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    for index in 0..3 {
        let task = access
            .create_task(task_spec(&format!("completed-{index}")), index + 2)
            .unwrap()
            .value;
        access
            .transition(task.id(), TaskStatus::Completed, index + 10)
            .unwrap();
    }
    access.create_task(task_spec("pending"), 20).unwrap();

    let lines = task_status_lines(&access.list(), 2);

    assert_eq!(lines[0], "━━ Tasks: 3/4 ━━");
    assert_eq!(lines.len(), 4);
    assert!(lines[1].contains("completed-2"));
    assert!(lines[2].contains("pending"));
    assert_eq!(lines[3], "… +2 more");
}

#[test]
fn task_status_lines_returns_empty_when_line_limit_is_zero() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    access.create_task(task_spec("pending"), 2).unwrap();

    assert!(task_status_lines(&access.list(), 0).is_empty());
}

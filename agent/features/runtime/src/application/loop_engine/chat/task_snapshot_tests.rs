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
fn task_state_view_preserves_structured_fields() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    let created = access.create_task(task_spec("实现适配器"), 2).unwrap();

    let state = build_task_state_view(access, "session-a");

    assert_eq!(state.session_id, "session-a");
    assert_eq!(state.revision, access.revision().get());
    assert_eq!(
        state.current_batch.unwrap().summary.as_deref(),
        Some("batch")
    );
    assert_eq!(state.items[0].id, created.value.id().get());
    assert_eq!(state.items[0].sequence, 1);
    assert_eq!(state.items[0].subject, "实现适配器");
    assert_eq!(state.items[0].status, sdk::TaskItemStatusView::Pending);
    assert_eq!(state.items[0].priority, sdk::TaskPriorityView::Normal);
}

#[test]
fn task_state_view_empty_without_active_batch_keeps_revision() {
    let store = task::TaskStore::new();
    let access: &dyn TaskAccess = &store;
    let state = build_task_state_view(access, "session-b");

    assert_eq!(state.session_id, "session-b");
    assert_eq!(state.revision, access.revision().get());
    assert!(state.current_batch.is_none());
    assert!(state.items.is_empty());
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
    assert_eq!(lines[1], "✓ #1 completed");
    assert_eq!(lines[2], "■ #2 working");
    assert_eq!(lines[3], "□ #3 blocked (blocked by #1)");
}

#[test]
fn blocked_by_omits_dependencies_outside_current_batch() {
    let known = TaskId::new(1);
    let unknown = TaskId::new(u64::MAX);
    let display_map = [(known, 1_u64)].into_iter().collect();

    let rendered = format_blocked_by(&[known, unknown], &display_map);

    assert_eq!(rendered, " (blocked by #1)");
    assert!(!rendered.contains(&unknown.to_string()));
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

#[test]
fn task_reminder_intent_preserves_count_and_active_list() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    let completed = access.create_task(task_spec("done"), 2).unwrap().value;
    let _pending = access.create_task(task_spec("todo"), 3).unwrap().value;
    access
        .transition(completed.id(), TaskStatus::Completed, 4)
        .unwrap();

    let reminder = build_task_reminder_intent(access, 7).expect("reminder intent");
    let context::domain::InvocationReminder::TaskProgress(progress) = reminder else {
        panic!("expected task progress reminder");
    };

    assert_eq!(progress.completed, 1);
    assert_eq!(progress.total, 2);
    assert_eq!(progress.items.len(), 2);
    assert_eq!(progress.items[0].sequence, 1);
    assert_eq!(progress.items[0].subject, "done");
    assert_eq!(
        progress.items[0].status,
        context::domain::TaskProgressStatus::Completed
    );
    assert_eq!(progress.items[1].sequence, 2);
    assert_eq!(progress.items[1].subject, "todo");
    assert_eq!(
        progress.items[1].status,
        context::domain::TaskProgressStatus::Pending
    );
}

#[test]
fn task_reminder_intent_none_without_tasks() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    assert!(build_task_reminder_intent(access, 7).is_none());
}

#[test]
fn task_reminder_intent_none_without_active_batch() {
    let store = task::TaskStore::new();
    let access: &dyn TaskAccess = &store;
    assert!(build_task_reminder_intent(access, 7).is_none());
}

/// #1537：build_task_snapshot_text 返回纯文本（无标签包装），供 compact 拼接。
/// compact 路径携带完整标识（batch id / task id / seq），与 TUI 路径不同。
#[test]
fn task_snapshot_text_renders_with_full_identifiers_for_compact() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    let completed = access.create_task(task_spec("done"), 2).unwrap().value;
    let _pending = access.create_task(task_spec("todo"), 3).unwrap().value;
    access
        .transition(completed.id(), TaskStatus::Completed, 4)
        .unwrap();

    let text = build_task_snapshot_text(access).expect("snapshot text rendered");

    // 不含 TUI 标签包装
    assert!(!text.contains("<system-reminder>"));
    // 携带 batch 标题
    assert!(text.contains("Batch #"));
    assert!(text.contains("Tasks: 1/2"));
    // 携带完整标识：task id + seq
    assert!(text.contains("task:"));
    assert!(text.contains("seq:"));
    // 携带 subject
    assert!(text.contains("done"));
    assert!(text.contains("todo"));
    // ✓ 用于 Completed，□ 用于 Pending
    assert!(text.contains("✓"));
    assert!(text.contains("□"));
}

/// #1537：无活跃 task 时 build_task_snapshot_text 返回 None。
#[test]
fn task_snapshot_text_none_without_tasks() {
    let store = access_with_active_batch();
    let access: &dyn TaskAccess = &store;
    assert!(build_task_snapshot_text(access).is_none());
}

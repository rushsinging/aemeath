use super::*;
use std::sync::Arc;

fn store_with_empty_batch() -> Arc<task::TaskStore> {
    let store = Arc::new(task::TaskStore::new());
    store
        .create_batch(
            task::BatchCreateSpecData::try_new("batch".into()).unwrap(),
            1,
        )
        .unwrap();
    store
}

fn store_with_two_tasks() -> Arc<task::TaskStore> {
    let store = store_with_empty_batch();
    for index in 0..2 {
        store
            .create_task(
                task::TaskCreateSpecData::try_new(
                    format!("任务{index}"),
                    String::new(),
                    None,
                    task::TaskPriorityData::Normal,
                )
                .unwrap(),
                index + 2,
            )
            .unwrap();
    }
    store
}

#[test]
fn no_active_list_error_names_both_creation_tools_in_order() {
    let store = Arc::new(task::TaskStore::new());
    let text = current_list_task_not_found_text(store.as_ref(), "1");
    assert!(text.contains("no active task list"), "{text}");
    assert!(text.contains("TaskListCreate"), "{text}");
    assert!(text.contains("TaskCreate"), "{text}");
}

#[test]
fn empty_list_error_hints_list_is_not_a_task() {
    let store = store_with_empty_batch();
    let text = current_list_task_not_found_text(store.as_ref(), "1");
    assert!(text.contains("#1"), "{text}");
    assert!(text.contains("0 tasks"), "{text}");
    assert!(text.contains("TaskCreate"), "{text}");
}

#[test]
fn non_empty_list_error_lists_valid_task_ids() {
    let store = store_with_two_tasks();
    let text = current_list_task_not_found_text(store.as_ref(), "9");
    assert!(text.contains("valid task ids: 1, 2"), "{text}");
}

#[test]
fn explicit_list_error_keeps_list_id_prefix_and_context() {
    let store = store_with_empty_batch();
    let text = explicit_list_task_not_found_text(store.as_ref(), task::BatchIdData::new(1), "1");
    assert!(
        text.starts_with("Task not found in task list #1: 1"),
        "{text}"
    );
    assert!(text.contains("0 tasks"), "{text}");
}

#[test]
fn missing_snapshot_error_points_to_list_discovery_tools() {
    let store = store_with_empty_batch();
    let text = explicit_list_task_not_found_text(store.as_ref(), task::BatchIdData::new(7), "1");
    assert!(text.contains("task list not found"), "{text}");
    assert!(text.contains("TaskLists"), "{text}");
}

use super::build_task_window;
use super::helpers_tests::{make_display_map, make_task_with_ts};
use sdk::{TaskState, TaskSummary};

#[test]
fn test_completed_group_sorted_by_updated_at_asc_and_window_keeps_previous_completed() {
    let tasks = vec![
        make_task_with_ts("1", "step one", TaskState::Completed, 100),
        make_task_with_ts("2", "step two", TaskState::Completed, 300),
        make_task_with_ts("3", "step three", TaskState::Completed, 500),
        make_task_with_ts("4", "current", TaskState::InProgress, 400),
        make_task_with_ts("5", "next", TaskState::Pending, 600),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #3 step three")); // ts=500
    assert!(result[2].contains("■ #4"));
    assert!(result[3].contains("□ #5"));
}

#[test]
fn test_window_feature_24_keeps_in_progress_and_pending_visible() {
    let mut tasks: Vec<TaskSummary> = (1..=5)
        .map(|i| {
            make_task_with_ts(
                &i.to_string(),
                &format!("done {}", i),
                TaskState::Completed,
                i * 100,
            )
        })
        .collect();
    tasks.push(make_task_with_ts("6", "doing", TaskState::InProgress, 600));
    tasks.push(make_task_with_ts("7", "pending", TaskState::Pending, 700));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 4);
    assert_eq!(result.len(), 5);
    assert!(result[1].contains("✓ #5 done 5"));
    assert!(result[2].contains("■ #6 doing"));
    assert!(result[3].contains("□ #7 pending"));
    assert!(result[4].contains("+4 more"));
}

#[test]
fn test_execution_order_reflected_in_display() {
    // Simulates 1→2→7→3→4 execution order; task 3 is the previous completed task.
    let tasks = vec![
        make_task_with_ts("1", "task 1", TaskState::Completed, 100),
        make_task_with_ts("2", "task 2", TaskState::Completed, 200),
        make_task_with_ts("3", "task 3", TaskState::Completed, 500),
        make_task_with_ts("4", "task 4", TaskState::InProgress, 600),
        make_task_with_ts("5", "task 5", TaskState::Pending, 700),
        make_task_with_ts("7", "task 7", TaskState::Completed, 400),
    ];
    // display map: sorted ids 1,2,3,4,5,7 → 1→#1, 2→#2, 3→#3, 4→#4, 5→#5, 7→#6
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #3 task 3"));
    assert!(result[2].contains("■ #4 task 4"));
    assert!(result[3].contains("□ #5 task 5"));
}

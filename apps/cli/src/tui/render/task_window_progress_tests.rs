use super::build_task_window;
use super::helpers_tests::{make_display_map, make_task_with_ts};
use sdk::{TaskState, TaskSummary};

#[test]
fn test_completed_group_sorted_by_updated_at_asc_and_within_max_shows_all_completed() {
    let tasks = vec![
        make_task_with_ts("1", "step one", TaskState::Completed, 100),
        make_task_with_ts("2", "step two", TaskState::Completed, 300),
        make_task_with_ts("3", "step three", TaskState::Completed, 500),
        make_task_with_ts("4", "current", TaskState::InProgress, 400),
        make_task_with_ts("5", "next", TaskState::Pending, 600),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #1 step one")); // ts=100
    assert!(result[2].contains("✓ #2 step two")); // ts=300
    assert!(result[3].contains("✓ #3 step three")); // ts=500
    assert!(result[4].contains("■ #4"));
    assert!(result[5].contains("□ #5"));
    assert!(!result.iter().any(|line| line.contains("more")));
}

#[test]
fn test_window_feature_24_keeps_in_progress_pending_visible_within_total_line_limit() {
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
    // task_slots_with_fold = 2 → 1 completed (reserved) + 1 in_progress
    assert_eq!(result.len(), 4);
    assert!(result[1].contains("✓ #5 done 5")); // most recent completed
    assert!(result[2].contains("■ #6 doing"));
    assert!(result[3].contains("+5 more"));
}

#[test]
fn test_execution_order_reflected_in_display_without_fold_within_max_lines() {
    // total <= max_lines 时按组完整显示；completed 组内仍按 updated_at 升序。
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
    assert!(result[1].contains("✓ #1 task 1"));
    assert!(result[2].contains("✓ #2 task 2"));
    assert!(result[3].contains("✓ #6 task 7"));
    assert!(result[4].contains("✓ #3 task 3"));
    assert!(result[5].contains("■ #4 task 4"));
    assert!(result[6].contains("□ #5 task 5"));
    assert!(!result.iter().any(|line| line.contains("more")));
}

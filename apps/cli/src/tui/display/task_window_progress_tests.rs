use super::build_task_window;
use super::helpers_tests::{make_display_map, make_task_with_ts};
use ::runtime::api::core::task::{Task, TaskStatus};

#[test]
fn test_completed_group_sorted_by_updated_at_desc() {
    let tasks = vec![
        make_task_with_ts("1", "step one", TaskStatus::Completed, 100),
        make_task_with_ts("2", "step two", TaskStatus::Completed, 300),
        make_task_with_ts("3", "step three", TaskStatus::Completed, 500),
        make_task_with_ts("4", "current", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "next", TaskStatus::Pending, 600),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #3 step three")); // ts=500
    assert!(result[2].contains("✓ #2 step two")); // ts=300
    assert!(result[3].contains("✓ #1 step one")); // ts=100
    assert!(result[4].contains("■ #4"));
    assert!(result[5].contains("□ #5"));
}

#[test]
fn test_window_truncates_across_groups() {
    let mut tasks: Vec<Task> = (1..=5)
        .map(|i| {
            make_task_with_ts(
                &i.to_string(),
                &format!("done {}", i),
                TaskStatus::Completed,
                i * 100,
            )
        })
        .collect();
    tasks.push(make_task_with_ts("6", "doing", TaskStatus::InProgress, 600));
    tasks.push(make_task_with_ts("7", "pending", TaskStatus::Pending, 700));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 4);
    // summary + 4 tasks + fold hint
    assert_eq!(result.len(), 6);
    // completed sorted by updated_at desc → #5, #4, #3, #2 (first 4)
    assert!(result[1].contains("✓ #5 done 5"));
    assert!(result[4].contains("✓ #2 done 2"));
    assert!(result[5].contains("+3 more"));
}

#[test]
fn test_execution_order_reflected_in_display() {
    // Simulates 1→2→7→3→4 execution order
    // Each completed at different timestamps
    let tasks = vec![
        make_task_with_ts("1", "task 1", TaskStatus::Completed, 100), // done first
        make_task_with_ts("2", "task 2", TaskStatus::Completed, 200),
        make_task_with_ts("3", "task 3", TaskStatus::Completed, 500), // done after 7
        make_task_with_ts("4", "task 4", TaskStatus::InProgress, 600), // currently running
        make_task_with_ts("5", "task 5", TaskStatus::Pending, 700),
        make_task_with_ts("7", "task 7", TaskStatus::Completed, 400), // done before 3
    ];
    // display map: sorted ids 1,2,3,4,5,7 → 1→#1, 2→#2, 3→#3, 4→#4, 5→#5, 7→#6
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // completed desc by updated_at: task 3(500), task 7(400), task 2(200), task 1(100)
    assert!(result[1].contains("✓ #3 task 3")); // display #3
    assert!(result[2].contains("✓ #6 task 7")); // display #6 (global id 7)
    assert!(result[3].contains("✓ #2 task 2"));
    assert!(result[4].contains("✓ #1 task 1"));
    // in_progress
    assert!(result[5].contains("■ #4 task 4"));
    // pending
    assert!(result[6].contains("□ #5 task 5"));
}

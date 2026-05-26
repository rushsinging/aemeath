use crate::tui::display::task_window_helpers_tests::{make_display_map, make_task, make_task_with_ts};
use super::*;
use ::runtime::api::core::task::{Task, TaskStatus};
use crate::tui::display::task_window::build_task_window;

#[test]
fn test_empty() {
    let result = build_task_window(&[], &Default::default(), 7);
    assert!(result.is_empty());
}

#[test]
fn test_max_lines_zero() {
    let tasks = vec![make_task("1", "test", TaskStatus::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 0);
    assert!(result.is_empty());
}

#[test]
fn test_single_pending() {
    let tasks = vec![make_task("1", "do thing", TaskStatus::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 2);
    assert!(result[0].contains("0/1"));
    assert!(result[1].contains("□ #1 do thing"));
}

#[test]
fn test_single_in_progress() {
    let tasks = vec![make_task("1", "in progress", TaskStatus::InProgress)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("■ #1"));
}

#[test]
fn test_single_completed() {
    let tasks = vec![make_task("1", "done", TaskStatus::Completed)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #1 done"));
}

#[test]
fn test_status_group_ordering() {
    // completed 按 updated_at 降序：task 7 (ts=700) → task 3 (ts=300) → task 1 (ts=100)
    // in_progress 按 updated_at 升序：task 4 (ts=400)
    // pending 按 display_number 升序：task 2 (display=2) → task 5 (display=5)
    let tasks = vec![
        make_task_with_ts("1", "done x", TaskStatus::Completed, 100),
        make_task_with_ts("2", "pending a", TaskStatus::Pending, 200),
        make_task_with_ts("3", "done y", TaskStatus::Completed, 300),
        make_task_with_ts("4", "doing a", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "pending b", TaskStatus::Pending, 500),
        make_task_with_ts("7", "done z", TaskStatus::Completed, 700),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 7); // summary + 6 tasks
    assert!(result[0].contains("3/6"));
    // completed group: updated_at desc → #6(done z,ts=700), #3(done y,ts=300), #1(done x,ts=100)
    assert!(result[1].contains("✓ #6 done z"));
    assert!(result[2].contains("✓ #3 done y"));
    assert!(result[3].contains("✓ #1 done x"));
    // in_progress group
    assert!(result[4].contains("■ #4 doing a"));
    // pending group: display_number asc → #2, #5
    assert!(result[5].contains("□ #2 pending a"));
    assert!(result[6].contains("□ #5 pending b"));
}

#[test]
fn test_truncation_with_fold_hint() {
    let tasks: Vec<Task> = (1..=20)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::Pending))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // summary + 7 tasks + fold hint
    assert_eq!(result.len(), 9);
    assert!(result[0].contains("0/20"));
    assert!(result.last().unwrap().contains("+13 more"));
}

#[test]
fn test_all_completed() {
    // make_task uses id as updated_at, so higher id = more recent → desc order
    let tasks: Vec<Task> = (1..=10)
        .map(|i| {
            make_task(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::Completed,
            )
        })
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 9); // summary + 7 + fold
    assert!(result[0].contains("10/10"));
    // completed sorted by updated_at desc → #10, #9, #8, ...
    assert!(result[1].contains("✓ #10"));
    assert!(result[2].contains("✓ #9"));
    assert!(result.last().unwrap().contains("+3 more"));
}

#[test]
fn test_display_numbers_match_store_numbering() {
    // Global ids are non-sequential: 8, 9, 10
    // Display numbers should be 1, 2, 3 (batch-local)
    let tasks = vec![
        make_task("8", "first", TaskStatus::Pending),
        make_task("9", "second", TaskStatus::InProgress),
        make_task("10", "third", TaskStatus::Completed),
    ];
    let map = make_display_map(&tasks);
    assert_eq!(map["8"], 1);
    assert_eq!(map["9"], 2);
    assert_eq!(map["10"], 3);
    let result = build_task_window(&tasks, &map, 7);
    // completed (id=10, display=3) → in_progress (id=9, display=2) → pending (id=8, display=1)
    assert!(result[1].contains("✓ #3 third"));
    assert!(result[2].contains("■ #2 second"));
    assert!(result[3].contains("□ #1 first"));
}

#[test]
fn test_deleted_tasks_excluded() {
    let tasks = vec![
        make_task("1", "done", TaskStatus::Completed),
        make_task("2", "deleted", TaskStatus::Deleted),
        make_task("3", "pending", TaskStatus::Pending),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    let task_lines: Vec<_> = result.iter().skip(1).collect();
    assert_eq!(task_lines.len(), 2);
    assert!(task_lines[0].contains("✓ #1"));
    assert!(task_lines[1].contains("□ #2"));
}

#[test]
fn test_owner_display() {
    let mut task = make_task("1", "owned task", TaskStatus::InProgress);
    task.owner = Some("agent-1".to_string());
    let tasks = vec![task];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("@agent-1"));
}

#[test]
fn test_window_truncates_across_groups() {
    let mut tasks: Vec<Task> = (1..=5)
        .map(|i| {
            make_task(
                &i.to_string(),
                &format!("done {}", i),
                TaskStatus::Completed,
            )
        })
        .collect();
    tasks.push(make_task("6", "doing", TaskStatus::InProgress));
    tasks.push(make_task("7", "pending", TaskStatus::Pending));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 4);
    // summary + 4 tasks + fold hint
    assert_eq!(result.len(), 6);
    // completed sorted by updated_at desc → #5, #4, #3, #2 (first 4 shown)
    assert!(result[1].contains("✓ #5 done 5"));
    assert!(result[4].contains("✓ #2 done 2"));
    assert!(result[5].contains("+3 more"));
}

#[test]
fn test_recent_completed_before_older() {
    let tasks = vec![
        make_task_with_ts("1", "old completed", TaskStatus::Completed, 100),
        make_task_with_ts("2", "middle completed", TaskStatus::Completed, 200),
        make_task_with_ts("3", "newest completed", TaskStatus::Completed, 300),
        make_task_with_ts("4", "current", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "next", TaskStatus::Pending, 500),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 3);
    // 5 ordered tasks, max_lines=3 → show 3 most recent completed + fold hint
    assert!(result[1].contains("✓ #3 newest completed"));
    assert!(result[2].contains("✓ #2 middle completed"));
    assert!(result[3].contains("✓ #1 old completed"));
    assert!(result[4].contains("+2 more"));
}

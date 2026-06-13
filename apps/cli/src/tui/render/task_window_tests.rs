use super::build_task_window;
use super::helpers_tests::{make_display_map, make_task, make_task_with_ts};
use sdk::{TaskState, TaskSummary};

#[test]
fn test_empty() {
    let result = build_task_window(&[], &Default::default(), 7);
    assert!(result.is_empty());
}

#[test]
fn test_max_lines_zero() {
    let tasks = vec![make_task("1", "test", TaskState::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 0);
    assert!(result.is_empty());
}

#[test]
fn test_single_pending() {
    let tasks = vec![make_task("1", "do thing", TaskState::Pending)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 2);
    assert!(result[0].contains("0/1"));
    assert!(result[1].contains("□ #1 do thing"));
}

#[test]
fn test_single_in_progress() {
    let tasks = vec![make_task("1", "in progress", TaskState::InProgress)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("■ #1"));
}

#[test]
fn test_single_completed() {
    let tasks = vec![make_task("1", "done", TaskState::Completed)];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("✓ #1 done"));
}

#[test]
fn test_status_group_ordering() {
    // total <= max_lines 时不折叠：completed 按 updated_at 升序完整显示；
    // in_progress 按 updated_at 升序；pending 按 display_number 升序。
    let tasks = vec![
        make_task_with_ts("1", "done x", TaskState::Completed, 100),
        make_task_with_ts("2", "pending a", TaskState::Pending, 200),
        make_task_with_ts("3", "done y", TaskState::Completed, 300),
        make_task_with_ts("4", "doing a", TaskState::InProgress, 400),
        make_task_with_ts("5", "pending b", TaskState::Pending, 500),
        make_task_with_ts("7", "done z", TaskState::Completed, 700),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert_eq!(result.len(), 7); // summary + all 6 active tasks
    assert!(result[0].contains("3/6"));
    assert!(result[1].contains("✓ #1 done x"));
    assert!(result[2].contains("✓ #3 done y"));
    assert!(result[3].contains("✓ #6 done z"));
    assert!(result[4].contains("■ #4 doing a"));
    assert!(result[5].contains("□ #2 pending a"));
    assert!(result[6].contains("□ #5 pending b"));
    assert!(!result.iter().any(|line| line.contains("more")));
}

#[test]
fn test_truncation_with_fold_hint() {
    let tasks: Vec<TaskSummary> = (1..=20)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskState::Pending))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // summary + 5 tasks + fold hint
    assert_eq!(result.len(), 7);
    assert!(result[0].contains("0/20"));
    assert!(result[1].contains("□ #1 task 1"));
    assert!(result[5].contains("□ #5 task 5"));
    assert!(result.last().unwrap().contains("+15 more"));
}

#[test]
fn test_all_completed_over_max_lines_backfills_recent_completed() {
    let tasks: Vec<TaskSummary> = (1..=10)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskState::Completed))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // task_slots_with_fold = 5 → show 5 most recent completed (newest first)
    assert_eq!(result.len(), 7); // summary + 5 completed + fold
    assert!(result[0].contains("10/10"));
    assert!(result[1].contains("✓ #10 task 10"));
    assert!(result[5].contains("✓ #6 task 6"));
    assert!(result.last().unwrap().contains("+5 more"));
}

#[test]
fn test_all_completed_at_window_limit_backfills_recent_completed() {
    let tasks: Vec<TaskSummary> = (1..=7)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskState::Completed))
        .collect();
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // task_slots_with_fold = 5 → show 5 most recent completed
    assert_eq!(result.len(), 7); // summary + 5 completed + fold
    assert!(result[0].contains("7/7"));
    assert!(result[1].contains("✓ #7 task 7"));
    assert!(result[5].contains("✓ #3 task 3"));
    assert!(result.last().unwrap().contains("+2 more"));
}

#[test]
fn test_display_numbers_match_store_numbering() {
    // Global ids are non-sequential: 8, 9, 10
    // Display numbers should be 1, 2, 3 (batch-local)
    let tasks = vec![
        make_task("8", "first", TaskState::Pending),
        make_task("9", "second", TaskState::InProgress),
        make_task("10", "third", TaskState::Completed),
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
        make_task("1", "done", TaskState::Completed),
        make_task("2", "deleted", TaskState::Deleted),
        make_task("3", "pending", TaskState::Pending),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[0].contains("1/2"));
    assert!(result[1].contains("✓ #1"));
    assert!(result[2].contains("□ #2"));
    assert!(!result.iter().any(|line| line.contains("deleted")));
}

#[test]
fn test_owner_display() {
    let mut task = make_task("1", "owned task", TaskState::InProgress);
    task.owner = Some("agent-1".to_string());
    let tasks = vec![task];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[1].contains("@agent-1"));
}

#[test]
fn test_over_max_lines_fills_in_progress_then_pending_then_completed_backfill() {
    let mut tasks = vec![make_task_with_ts(
        "1",
        "recent completed",
        TaskState::Completed,
        100,
    )];
    tasks.extend((2..=8).map(|i| {
        make_task_with_ts(
            &i.to_string(),
            &format!("doing {}", i),
            TaskState::InProgress,
            i * 100,
        )
    }));
    tasks.push(make_task_with_ts("9", "next", TaskState::Pending, 900));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    // task_slots_with_fold = 5 → 5 in_progress (earliest by updated_at)
    assert_eq!(result.len(), 7); // summary + 5 task lines + fold
    assert!(result[1].contains("■ #2 doing 2"));
    assert!(result[5].contains("■ #6 doing 6"));
    assert!(!result.iter().any(|line| line.contains("next")));
    assert!(result.last().unwrap().contains("+4 more"));
}

#[test]
fn test_window_prefers_in_progress_pending_then_completed_backfill() {
    let mut tasks: Vec<TaskSummary> = (1..=5)
        .map(|i| make_task(&i.to_string(), &format!("done {}", i), TaskState::Completed))
        .collect();
    tasks.push(make_task("6", "doing", TaskState::InProgress));
    tasks.push(make_task("7", "pending", TaskState::Pending));
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 4);
    // task_slots_with_fold = 2 → in_progress #6 + pending #7
    assert_eq!(result.len(), 4); // summary + in_progress + pending + fold hint
    assert!(result[1].contains("■ #6 doing"));
    assert!(result[2].contains("□ #7 pending"));
    assert!(result[3].contains("+5 more"));
}

#[test]
fn test_tight_window_prefers_in_progress_over_completed() {
    let tasks = vec![
        make_task_with_ts("1", "old completed", TaskState::Completed, 100),
        make_task_with_ts("2", "middle completed", TaskState::Completed, 200),
        make_task_with_ts("3", "newest completed", TaskState::Completed, 300),
        make_task_with_ts("4", "current", TaskState::InProgress, 400),
        make_task_with_ts("5", "next", TaskState::Pending, 500),
    ];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 3);
    // task_slots_with_fold = 1 → in_progress #4
    assert_eq!(result.len(), 3); // summary + in_progress + fold
    assert!(result[1].contains("■ #4 current"));
    assert!(result[2].contains("+4 more"));
}

#[test]
fn test_blocked_by_display_uses_batch_local_numbers() {
    let completed = make_task_with_ts("1", "setup", TaskState::Completed, 100);
    let mut pending = make_task_with_ts("2", "follow up", TaskState::Pending, 200);
    pending.blocked_by = vec!["1".to_string(), "999".to_string()];
    let tasks = vec![completed, pending];
    let map = make_display_map(&tasks);
    let result = build_task_window(&tasks, &map, 7);
    assert!(result[2].contains("□ #2 follow up (blocked by #1, #999)"));
}

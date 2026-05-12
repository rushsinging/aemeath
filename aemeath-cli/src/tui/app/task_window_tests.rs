use super::*;

fn make_task_with_ts(id: &str, subject: &str, status: TaskStatus, ts: u64) -> Task {
    Task {
        id: id.to_string(),
        subject: subject.to_string(),
        description: String::new(),
        status,
        active_form: None,
        owner: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        priority: aemeath_core::task::TaskPriority::Normal,
        progress: 0,
        progress_message: None,
        created_at: ts,
        updated_at: ts,
        session_id: None,
        tags: Vec::new(),
        batch: 0,
    }
}

fn make_task(id: &str, subject: &str, status: TaskStatus) -> Task {
    make_task_with_ts(id, subject, status, id.parse::<u64>().unwrap_or(100))
}

#[test]
fn test_build_task_window_empty() {
    let result = build_task_window(&[], 7, 1);
    assert!(result.is_empty());
}

#[test]
fn test_build_task_window_max_lines_zero() {
    let tasks = vec![make_task("1", "test", TaskStatus::Pending)];
    let result = build_task_window(&tasks, 0, 1);
    assert!(result.is_empty());
}

#[test]
fn test_build_task_window_single_pending() {
    let tasks = vec![make_task("1", "do thing", TaskStatus::Pending)];
    let result = build_task_window(&tasks, 7, 1);
    assert_eq!(result.len(), 2); // summary + 1 task
    assert!(result[0].contains("0/1"));
    assert!(result[1].contains("□ #1 do thing"));
}

#[test]
fn test_build_task_window_single_in_progress() {
    let tasks = vec![make_task("1", "in progress", TaskStatus::InProgress)];
    let result = build_task_window(&tasks, 7, 1);
    assert!(result[1].contains("■ #1"));
}

#[test]
fn test_build_task_window_single_completed() {
    let tasks = vec![make_task("1", "done", TaskStatus::Completed)];
    let result = build_task_window(&tasks, 7, 1);
    assert!(result[1].contains("✓ #1 done"));
}

#[test]
fn test_build_task_window_mix() {
    let tasks = vec![
        make_task("1", "done a", TaskStatus::Completed),
        make_task("2", "done b", TaskStatus::Completed),
        make_task("3", "doing c", TaskStatus::InProgress),
        make_task("4", "pending d", TaskStatus::Pending),
        make_task("5", "pending e", TaskStatus::Pending),
    ];
    let result = build_task_window(&tasks, 7, 1);
    // 温和扩展会补充额外的 completed → summary + 2 completed + in_progress + 2 pending = 6
    assert_eq!(result.len(), 6);
    assert!(result[0].contains("2/5"));
    assert!(result[1].contains("✓ #1 done a")); // completed 按 task id 升序
    assert!(result[2].contains("✓ #2 done b")); // extra completed inserted after main completed
    assert!(result[3].contains("■ #3 doing c")); // in_progress
    assert!(result[4].contains("□ #4"));
    assert!(result[5].contains("□ #5"));
}

#[test]
fn test_build_task_window_all_completed() {
    let tasks: Vec<_> = (1..=10)
        .map(|i| {
            make_task(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::Completed,
            )
        })
        .collect();
    let result = build_task_window(&tasks, 7, 1);
    // summary + 7 completed + fold hint = 9 lines
    assert_eq!(result.len(), 9);
    assert!(result[0].contains("10/10"));
    assert!(result.last().unwrap().contains("+3 more completed"));
}

#[test]
fn test_build_task_window_overflow_pending() {
    let tasks: Vec<_> = (1..=20)
        .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::Pending))
        .collect();
    let result = build_task_window(&tasks, 7, 1);
    // summary + 7 pending + fold
    assert_eq!(result.len(), 9);
    assert!(result[0].contains("0/20"));
    assert!(result.last().unwrap().contains("+13 more"));
}

#[test]
fn test_build_task_window_in_progress_overflow() {
    let mut tasks: Vec<_> = (1..=10)
        .map(|i| {
            make_task(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::InProgress,
            )
        })
        .collect();
    tasks.push(make_task("11", "pending", TaskStatus::Pending));
    let result = build_task_window(&tasks, 7, 1);
    // summary + 7 in_progress (pending falls off) + fold
    assert_eq!(result.len(), 9);
    assert!(result.last().unwrap().contains("+4 more"));
}

#[test]
fn test_build_task_window_no_in_progress() {
    let tasks = vec![
        make_task("1", "done a", TaskStatus::Completed),
        make_task("2", "pending b", TaskStatus::Pending),
        make_task("3", "pending c", TaskStatus::Pending),
        make_task("4", "pending d", TaskStatus::Pending),
    ];
    let result = build_task_window(&tasks, 7, 1);
    // summary + 1 completed + 3 pending = 5
    assert_eq!(result.len(), 5);
    assert!(result[0].contains("1/4"));
    assert!(result[1].contains("✓ #1"));
    assert!(result[2].contains("□ #2"));
}

// --- Bug #32 新增测试 ---

#[test]
fn test_lower_bound_serial_execution() {
    // 场景：10 tasks, 1 in_progress, 9 pending, 0 completed
    // 期望 >= 3 条 task（不含摘要）
    let mut tasks: Vec<_> = Vec::new();
    for i in 1..=10 {
        tasks.push(make_task(
            &i.to_string(),
            &format!("task {}", i),
            TaskStatus::Pending,
        ));
    }
    tasks[8].status = TaskStatus::InProgress; // #9 in_progress
    let result = build_task_window(&tasks, 7, 1);
    let task_lines = result.len() - 1;
    assert!(task_lines >= 3, "task_lines={}, expected >= 3", task_lines);
    assert!(result[0].contains("0/10"));
}

#[test]
fn test_lower_bound_with_completed_fill() {
    // 场景：10 tasks, 8 completed, 1 in_progress, 1 pending
    // show_last_completed=1 → 只有 1 completed
    // 下限保护应该补充更多 completed
    let mut tasks: Vec<_> = Vec::new();
    for i in 1..=10 {
        let status = match i {
            1..=8 => TaskStatus::Completed,
            9 => TaskStatus::InProgress,
            _ => TaskStatus::Pending,
        };
        tasks.push(make_task(&i.to_string(), &format!("task {}", i), status));
    }
    let result = build_task_window(&tasks, 7, 1);
    let task_lines = result.len() - 1;
    assert!(task_lines >= 3, "task_lines={}, expected >= 3", task_lines);
    // 应该显示不止 1 条 completed
    let comp_count = result.iter().filter(|l| l.starts_with('✓')).count();
    assert!(
        comp_count >= 2,
        "expected >= 2 completed, got {}",
        comp_count
    );
}

#[test]
fn test_pending_sequential_order() {
    // pending 应该按 id 升序显示，不跳跃
    let tasks = vec![
        make_task("10", "skip early", TaskStatus::Pending),
        make_task("2", "first", TaskStatus::Pending),
        make_task("5", "second", TaskStatus::Pending),
        make_task("3", "in progress", TaskStatus::InProgress),
    ];
    let result = build_task_window(&tasks, 7, 1);
    // summary + in_progress + 3 pending = 5
    assert_eq!(result.len(), 5);
    assert!(result[1].contains("■ #3 in progress")); // in_progress
    assert!(result[2].contains("□ #2 first")); // smallest id first
    assert!(result[3].contains("□ #5 second"));
    assert!(result[4].contains("□ #10 skip early"));
}

#[test]
fn test_completed_lines_keep_task_id_order_when_expanded() {
    let tasks = vec![
        make_task_with_ts(
            "1",
            "检查 bug 35 与 worktree 约定",
            TaskStatus::Completed,
            100,
        ),
        make_task_with_ts(
            "2",
            "创建 bug35 worktree 并验证基线",
            TaskStatus::Completed,
            300,
        ),
        make_task_with_ts("3", "定位 bug 35 根因", TaskStatus::Completed, 200),
        make_task_with_ts(
            "4",
            "添加回归测试并修复 bug 35",
            TaskStatus::InProgress,
            400,
        ),
        make_task_with_ts("5", "验证并更新文档", TaskStatus::Pending, 500),
    ];

    let result = build_task_window(&tasks, 7, 1);

    assert!(result[1].contains("✓ #1 检查 bug 35 与 worktree 约定"));
    assert!(result[2].contains("✓ #2 创建 bug35 worktree 并验证基线"));
    assert!(result[3].contains("✓ #3 定位 bug 35 根因"));
    assert!(result[4].contains("■ #4 添加回归测试并修复 bug 35"));
    assert!(result[5].contains("□ #5 验证并更新文档"));
}

#[test]
fn test_fold_hint_counts_only_unshown_tasks() {
    let tasks = vec![
        make_task("1", "done", TaskStatus::Completed),
        make_task("2", "doing", TaskStatus::InProgress),
        make_task("3", "pending a", TaskStatus::Pending),
        make_task("4", "pending b", TaskStatus::Pending),
        make_task("5", "pending c", TaskStatus::Pending),
    ];

    let result = build_task_window(&tasks, 4, 1);

    assert_eq!(result.len(), 6);
    assert!(result[1].contains("✓ #1 done"));
    assert!(result[2].contains("■ #2 doing"));
    assert!(result[3].contains("□ #3 pending a"));
    assert!(result[4].contains("□ #4 pending b"));
    assert!(result[5].contains("+1 more pending"));
}

#[test]
fn test_completed_ttl_excludes_old() {
    // TTL only applies when completed count exceeds max_lines.
    // With max_lines=7 and only 2 completed, TTL does NOT filter → both shown.
    let now: u64 = 10000;
    let tasks = vec![
        make_task_with_ts("1", "old done", TaskStatus::Completed, now - 3600),
        make_task_with_ts("2", "recent done", TaskStatus::Completed, now - 5),
        make_task_with_ts("3", "in progress", TaskStatus::InProgress, now),
        make_task_with_ts("4", "pending", TaskStatus::Pending, now),
    ];
    let result = build_task_window(&tasks, 7, 1);
    // Summary uses all_completed_count (2), not TTL-filtered
    assert!(result[0].contains("2/4"));
    // Both completed shown (within max_lines, no TTL filtering)
    assert!(result.iter().any(|l| l.contains("✓ #2")));
    assert!(result.iter().any(|l| l.contains("✓ #1")));

    // Now test with many completed (> max_lines) where TTL kicks in
    let mut many_tasks: Vec<Task> = Vec::new();
    for i in 0..10 {
        let ts = if i < 5 { now - 600 } else { now - 5 }; // first 5 are old
        many_tasks.push(make_task_with_ts(
            &format!("{}", i),
            &format!("task {}", i),
            TaskStatus::Completed,
            ts,
        ));
    }
    many_tasks.push(make_task_with_ts("10", "pending", TaskStatus::Pending, now));
    let result2 = build_task_window(&many_tasks, 7, 1);
    // Summary still shows all completed
    assert!(result2[0].contains("10/11"));
    // Old completed (0..4) should be filtered by TTL
    assert!(!result2.iter().any(|l| l.contains("✓ #0 ")));
}

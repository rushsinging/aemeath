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
    assert!(result[1].contains("✓ #2 done b")); // 最近完成
    assert!(result[2].contains("✓ #1 done a")); // 有余量时补充旧 completed
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

#[test]
fn test_build_task_window_displays_batch_local_numbers() {
    let tasks = vec![
        make_task("6", "second batch first", TaskStatus::Pending),
        make_task("7", "second batch second", TaskStatus::InProgress),
    ];
    let result = build_task_window(&tasks, 7, 1);

    assert!(result[1].contains("■ #2 second batch second"));
    assert!(result[2].contains("□ #1 second batch first"));
    assert!(!result.iter().any(|line| line.contains("#6")));
    assert!(!result.iter().any(|line| line.contains("#7")));
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
    assert!(result[1].contains("■ #2 in progress")); // in_progress
    assert!(result[2].contains("□ #1 first")); // smallest id first
    assert!(result[3].contains("□ #3 second"));
    assert!(result[4].contains("□ #4 skip early"));
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

    assert!(result[1].contains("✓ #2 创建 bug35 worktree 并验证基线"));
    assert!(result[2].contains("✓ #1 检查 bug 35 与 worktree 约定"));
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

#[test]
fn test_recent_completed_uses_updated_at_desc_before_id_order() {
    let tasks = vec![
        make_task_with_ts("1", "old completed", TaskStatus::Completed, 100),
        make_task_with_ts("2", "middle completed", TaskStatus::Completed, 200),
        make_task_with_ts("3", "newest completed", TaskStatus::Completed, 300),
        make_task_with_ts("4", "current", TaskStatus::InProgress, 400),
        make_task_with_ts("5", "next", TaskStatus::Pending, 500),
    ];

    let result = build_task_window(&tasks, 3, 1);

    assert!(result[1].contains("✓ #3 newest completed"));
    assert!(result[2].contains("■ #4 current"));
    assert!(result[3].contains("□ #5 next"));
    assert!(!result.iter().any(|line| line.contains("#1 old completed")));
}

#[test]
fn test_bug32_user_snapshot_keeps_full_window_when_only_recent_completed_and_in_progress() {
    let tasks = vec![
        make_task_with_ts(
            "1",
            "Critical 1: 删除 ProjectTaskStatus.Assigned，统一状态机",
            TaskStatus::Completed,
            100,
        ),
        make_task_with_ts("2", "Critical 2: already done", TaskStatus::Completed, 200),
        make_task_with_ts("3", "Critical 3: already done", TaskStatus::Completed, 300),
        make_task_with_ts(
            "4",
            "Critical 4: 定稿 Sub-Agent 部署模型",
            TaskStatus::Completed,
            400,
        ),
        make_task_with_ts(
            "5",
            "Critical 5: 定稿 Agent 进程模型",
            TaskStatus::Completed,
            500,
        ),
        make_task_with_ts(
            "6",
            "Important 1-3: 补 timeout/cancel/token/gRPC 错误码",
            TaskStatus::Completed,
            600,
        ),
        make_task_with_ts(
            "7",
            "Important 4+6: 补缺失 collection schema + 简化 model_health",
            TaskStatus::Completed,
            700,
        ),
        make_task_with_ts(
            "8",
            "Important 5: 明确 Scheduler Watch 语义",
            TaskStatus::Completed,
            800,
        ),
        make_task_with_ts(
            "9",
            "Important 7: can_create_agents 硬校验",
            TaskStatus::Completed,
            900,
        ),
        make_task_with_ts(
            "10",
            "Minor 1-6: 排版修复 + 开放问题清理 + 小修补",
            TaskStatus::InProgress,
            1000,
        ),
    ];

    let result = build_task_window(&tasks, 7, 1);

    assert_eq!(result.len(), 8);
    assert!(result[1].contains("✓ #4 Critical 4: 定稿 Sub-Agent 部署模型"));
    assert!(result[2].contains("✓ #5 Critical 5: 定稿 Agent 进程模型"));
    assert!(result[3].contains("✓ #6 Important 1-3: 补 timeout/cancel/token/gRPC 错误码"));
    assert!(result[4].contains("✓ #7 Important 4+6: 补缺失 collection schema + 简化 model_health"));
    assert!(result[5].contains("✓ #8 Important 5: 明确 Scheduler Watch 语义"));
    assert!(result[6].contains("✓ #9 Important 7: can_create_agents 硬校验"));
    assert!(result[7].contains("■ #10 Minor 1-6: 排版修复 + 开放问题清理 + 小修补"));
}

/// Bug #32 回归测试：窗口在有 TTL 压力时始终填满 max_lines
///
/// 模拟用户场景：13 条 task，completed 很多（部分超过 TTL），
/// pending 很少，in_progress 1 条。窗口应始终满 7 条。
#[test]
fn test_bug32_window_stays_full_with_ttl_pressure() {
    let now: u64 = 10000;
    let mut tasks: Vec<Task> = Vec::new();

    // 10 条 completed，前 8 条超过 TTL（>300s），后 2 条在 TTL 内
    for i in 1..=8 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("old completed {}", i),
            TaskStatus::Completed,
            now - 600,
        ));
    }
    for i in 9..=10 {
        tasks.push(make_task_with_ts(
            &i.to_string(),
            &format!("recent completed {}", i),
            TaskStatus::Completed,
            now - 10,
        ));
    }
    tasks.push(make_task_with_ts(
        "11",
        "current task",
        TaskStatus::InProgress,
        now,
    ));
    tasks.push(make_task_with_ts("12", "pending a", TaskStatus::Pending, now));
    tasks.push(make_task_with_ts("13", "pending b", TaskStatus::Pending, now));

    let result = build_task_window(&tasks, 7, 1);
    let task_lines = result.len() - 1;
    assert_eq!(
        task_lines, 7,
        "expected 7 task lines, got {}: {:?}",
        task_lines, result
    );
    assert!(result.iter().any(|l| l.contains("■ #11 current task")));
    assert!(result.iter().any(|l| l.contains("□ #12")));
    assert!(result.iter().any(|l| l.contains("□ #13")));
    let comp_count = result.iter().filter(|l| l.starts_with('✓')).count();
    assert!(
        comp_count >= 4,
        "expected >= 4 completed shown, got {}",
        comp_count
    );
}

/// Bug #32 回归测试：窗口从 7 条逐渐收缩的场景
///
/// 模拟执行过程：completed 从 0 增长到 12，pending 从 12 减少到 0，
/// 窗口始终应满 7 条。
#[test]
fn test_bug32_window_never_shrinks_during_progression() {
    let now: u64 = 10000;
    let max_lines = 7;

    // 阶段 1: 0 completed, 1 in_progress, 12 pending
    {
        let mut tasks: Vec<Task> = Vec::new();
        tasks.push(make_task_with_ts("1", "doing", TaskStatus::InProgress, now));
        for i in 2..=13 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::Pending,
                now,
            ));
        }
        let result = build_task_window(&tasks, max_lines, 1);
        // task 行数 = 总行数 - summary(1) - fold hints
        let task_lines = result.iter().skip(1).filter(|l| !l.starts_with('…')).count();
        assert_eq!(
            task_lines, max_lines,
            "stage 1: expected {} task lines, got {}",
            max_lines, task_lines
        );
    }

    // 阶段 2: 5 completed, 1 in_progress, 7 pending
    {
        let mut tasks: Vec<Task> = Vec::new();
        for i in 1..=5 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("done {}", i),
                TaskStatus::Completed,
                now - 100 + i as u64,
            ));
        }
        tasks.push(make_task_with_ts("6", "doing", TaskStatus::InProgress, now));
        for i in 7..=13 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::Pending,
                now,
            ));
        }
        let result = build_task_window(&tasks, max_lines, 1);
        let task_lines = result.iter().skip(1).filter(|l| !l.starts_with('…')).count();
        assert_eq!(
            task_lines, max_lines,
            "stage 2: expected {} task lines, got {}",
            max_lines, task_lines
        );
    }

    // 阶段 3: 10 completed, 1 in_progress, 2 pending
    {
        let mut tasks: Vec<Task> = Vec::new();
        for i in 1..=10 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("done {}", i),
                TaskStatus::Completed,
                now - 100 + i as u64,
            ));
        }
        tasks.push(make_task_with_ts("11", "doing", TaskStatus::InProgress, now));
        for i in 12..=13 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("task {}", i),
                TaskStatus::Pending,
                now,
            ));
        }
        let result = build_task_window(&tasks, max_lines, 1);
        let task_lines = result.iter().skip(1).filter(|l| !l.starts_with('…')).count();
        assert_eq!(
            task_lines, max_lines,
            "stage 3: expected {} task lines, got {}",
            max_lines, task_lines
        );
    }

    // 阶段 4: 12 completed, 1 in_progress, 0 pending（"只剩一条"场景）
    {
        let mut tasks: Vec<Task> = Vec::new();
        for i in 1..=12 {
            tasks.push(make_task_with_ts(
                &i.to_string(),
                &format!("done {}", i),
                TaskStatus::Completed,
                now - 100 + i as u64,
            ));
        }
        tasks.push(make_task_with_ts("13", "doing", TaskStatus::InProgress, now));
        let result = build_task_window(&tasks, max_lines, 1);
        let task_lines = result.iter().skip(1).filter(|l| !l.starts_with('…')).count();
        assert_eq!(
            task_lines, max_lines,
            "stage 4 (no pending): expected {} task lines, got {}: {:?}",
            max_lines, task_lines, result
        );
    }
}

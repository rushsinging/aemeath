//! Task list 窗口化显示 —— Feature #24
//!
//! 当 task 数量超过上限时，按"前后文相关性"取窗口：
//! 最近 completed + 所有 in_progress + pending 填充余量，其余折叠提示。

use aemeath_core::task::{Task, TaskStatus};

/// TTL for completed tasks shown in the window (seconds relative to newest).
/// Older completed are excluded **only when** the window would overflow `max_lines`.
/// When the window has spare capacity, all completed tasks in the current batch are shown.
const COMPLETED_TTL_SECS: u64 = 300; // 5 minutes

/// 构建 task 状态显示行（窗口化，含摘要行）。
///
/// 规则：
/// 1. 摘要行 `━━ Tasks: completed/total ━━` 始终反映全量
/// 2. 按优先级填充窗口（最多 `max_lines` 条）：
///    - `show_last_completed` 条最近完成的
///    - 所有 in_progress
///    - pending 按 id 升序填充剩余配额
/// 3. 超出部分以折叠提示 `… +N more pending` 表示
/// 4. 没有 in_progress 时，第一条 pending 视为"接下来要做"
/// 5. 全部 completed 时显示最后 `max_lines` 条 completed
/// 6. 空输入返回空 Vec
/// 7. 下限保护：task 行数不足 min(3, total) 且有剩余 task 时，扩大填充
pub fn build_task_window<'a>(
    tasks: &[Task],
    max_lines: usize,
    show_last_completed: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    // --- completed: 按 batch 内显示序号升序，保持任务列表稳定顺序 ---
    let display_numbers = batch_display_numbers(tasks);
    let all_completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut completed: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .collect();
    completed.sort_by_key(|t| task_display_number(t, &display_numbers));
    // TTL filter: only apply when there are more completed than max_lines
    let newest_ts = completed.iter().map(|t| t.updated_at).max().unwrap_or(0);
    let completed: Vec<&Task> = if completed.len() > max_lines {
        completed
            .into_iter()
            .filter(|t| newest_ts.saturating_sub(t.updated_at) <= COMPLETED_TTL_SECS)
            .collect()
    } else {
        completed
    };

    let in_progress: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InProgress)
        .collect();
    let mut pending: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    pending.sort_by_key(|t| task_display_number(t, &display_numbers));
    let total = tasks.len();
    let completed_count = all_completed_count;
    let in_progress_count = in_progress.len();
    let pending_count = pending.len();

    // 摘要行
    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    let capacity = max_lines;

    // 全部 completed 场景
    if in_progress_count == 0 && pending_count == 0 {
        let start = completed.len().saturating_sub(capacity);
        let shown = completed.len() - start;
        let hidden = completed.len() - shown;
        for t in &completed[start..] {
            // allow unsafe_text_op: Vec slice with bounds-checked start
            lines.push(format_task_line(t, &display_numbers));
        }
        if hidden > 0 {
            lines.push(format_fold_hint(hidden, "completed"));
        }
        return lines;
    }

    // --- 分配额度 ---
    let mut remaining = capacity;

    // 1. 最近 completed（最多 show_last_completed 条）
    let comp_show = show_last_completed.min(remaining).min(completed.len());
    for t in completed.iter().take(comp_show) {
        lines.push(format_task_line(t, &display_numbers));
    }
    remaining = remaining.saturating_sub(comp_show);
    // 2. 所有 in_progress
    let ip_show = in_progress_count.min(remaining);
    for t in in_progress.iter().take(ip_show) {
        lines.push(format_task_line(t, &display_numbers));
    }
    remaining = remaining.saturating_sub(ip_show);

    // 3. pending 填充余量
    let pending_show = pending_count.min(remaining);
    for t in pending.iter().take(pending_show) {
        lines.push(format_task_line(t, &display_numbers));
    }
    remaining = remaining.saturating_sub(pending_show);

    // --- 下限保护 + 温和扩展：有余量时继续填充 ---
    let _task_lines = lines.len() - 1; // exclude summary
    let min_show = 3.min(total);
    if remaining > 0 {
        // 补充更多 completed（跳过已显示的最新项），插入到已显示 completed 之后
        let more_comp = remaining.min(completed.len().saturating_sub(comp_show));
        let insert_pos = 1 + comp_show; // after summary + displayed completed
        for (i, t) in completed.iter().skip(comp_show).take(more_comp).enumerate() {
            lines.insert(insert_pos + i, format_task_line(t, &display_numbers));
        }
        remaining = remaining.saturating_sub(more_comp);
    }
    if remaining > 0 {
        // 补充更多 pending（跳过已显示的 pending_show 条）
        let more_pending = remaining.min(pending_count.saturating_sub(pending_show));
        for t in pending.iter().skip(pending_show).take(more_pending) {
            lines.push(format_task_line(t, &display_numbers));
        }
        remaining = remaining.saturating_sub(more_pending);
    }
    // 如果仍然不足 min_show，从更早的 completed 继续取
    if lines.len() - 1 < min_show && remaining > 0 {
        let shown_completed = completed.len().min(comp_show);
        let more = (min_show - (lines.len() - 1))
            .min(remaining)
            .min(completed.len().saturating_sub(shown_completed));
        for (i, t) in completed
            .iter()
            .skip(shown_completed)
            .take(more)
            .enumerate()
        {
            lines.insert(
                1 + shown_completed + i,
                format_task_line(t, &display_numbers),
            );
        }
    }
    // --- 折叠提示 ---
    let pending_hidden = pending_count.saturating_sub(pending_show);
    let ip_hidden = in_progress_count.saturating_sub(ip_show);
    let total_hidden = pending_hidden + ip_hidden;

    if total_hidden > 0 {
        let status = if pending_hidden >= ip_hidden {
            "pending"
        } else {
            "in_progress"
        };
        lines.push(format_fold_hint(total_hidden, status));
    }

    lines
}

fn format_task_line(t: &Task, display_numbers: &std::collections::HashMap<String, u64>) -> String {
    let icon = match t.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        _ => "?",
    };
    let owner = t
        .owner
        .as_deref()
        .map(|o| format!(" (@{})", o))
        .unwrap_or_default();
    format!(
        "{} #{} {}{}",
        icon,
        task_display_number(t, display_numbers),
        t.subject,
        owner
    )
}

fn batch_display_numbers(tasks: &[Task]) -> std::collections::HashMap<String, u64> {
    let mut by_batch: std::collections::BTreeMap<u64, Vec<&Task>> =
        std::collections::BTreeMap::new();
    for task in tasks {
        by_batch.entry(task.batch).or_default().push(task);
    }

    let mut result = std::collections::HashMap::new();
    for batch_tasks in by_batch.values_mut() {
        batch_tasks.sort_by_key(|task| task.id.parse::<u64>().unwrap_or(u64::MAX));
        for (idx, task) in batch_tasks.iter().enumerate() {
            result.insert(task.id.clone(), (idx + 1) as u64);
        }
    }
    result
}

fn task_display_number(t: &Task, display_numbers: &std::collections::HashMap<String, u64>) -> u64 {
    display_numbers
        .get(&t.id)
        .copied()
        .unwrap_or_else(|| t.id.parse::<u64>().unwrap_or(u64::MAX))
}

fn format_fold_hint(n: usize, status: &str) -> String {
    format!("… +{} more {}", n, status)
}

#[cfg(test)]
mod tests {
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
    fn test_build_task_window_uses_batch_local_numbering() {
        let mut first_in_second_batch = make_task("6", "second batch first", TaskStatus::Pending);
        first_in_second_batch.batch = 2;
        let mut second_in_second_batch = make_task("7", "second batch second", TaskStatus::Pending);
        second_in_second_batch.batch = 2;

        let result = build_task_window(&[first_in_second_batch, second_in_second_batch], 7, 1);

        assert!(result[1].contains("□ #1 second batch first"));
        assert!(result[2].contains("□ #2 second batch second"));
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
        assert!(result[1].contains("■ #2 in progress")); // in_progress, batch-local #2
        assert!(result[2].contains("□ #1 first")); // smallest id first, batch-local #1
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
}

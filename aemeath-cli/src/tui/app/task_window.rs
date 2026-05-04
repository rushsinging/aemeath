//! Task list 窗口化显示 —— Feature #24
//!
//! 当 task 数量超过上限时，按"前后文相关性"取窗口：
//! 最近 completed + 所有 in_progress + pending 填充余量，其余折叠提示。

use aemeath_core::task::{Task, TaskStatus};

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
pub fn build_task_window<'a>(
    tasks: &[Task],
    max_lines: usize,
    show_last_completed: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let completed: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .collect();
    let in_progress: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InProgress)
        .collect();
    // pending: 按 id 数字排序
    let mut pending: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    pending.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));

    let total = tasks.len();
    let completed_count = completed.len();
    let in_progress_count = in_progress.len();
    let pending_count = pending.len();

    // 摘要行
    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    // 摘要行占 1 行，剩余 max_lines 条 task
    let capacity = max_lines;

    if in_progress_count == 0 && pending_count == 0 {
        // 全部 completed：显示最后 capacity 条
        let start = if completed.len() > capacity {
            completed.len() - capacity
        } else {
            0
        };
        let shown = completed.len() - start;
        let hidden = completed.len() - shown;
        for t in &completed[start..] {
            lines.push(format_task_line(t));
        }
        if hidden > 0 {
            lines.push(format_fold_hint(hidden, "completed"));
        }
        return lines;
    }

    // 分配额度
    let mut remaining = capacity;

    // 1. 最近 completed（上一条）
    let last_completed_show = show_last_completed.min(remaining).min(completed.len());
    if last_completed_show > 0 {
        let start = completed.len() - last_completed_show;
        for t in &completed[start..] {
            lines.push(format_task_line(t));
        }
        remaining -= last_completed_show;
    }

    // 2. 所有 in_progress
    let ip_show = in_progress_count.min(remaining);
    for t in in_progress.iter().take(ip_show) {
        lines.push(format_task_line(t));
    }
    remaining -= ip_show;

    // 3. pending 填充余量
    let pending_show = pending_count.min(remaining);
    for t in pending.iter().take(pending_show) {
        lines.push(format_task_line(t));
    }
    let pending_hidden = pending_count - pending_show;
    let ip_hidden = if in_progress_count > ip_show {
        in_progress_count - ip_show
    } else {
        0
    };
    // comp_hidden not counted: extra completed are intentionally omitted per design,
    // not because of capacity overflow. Only count ip/pending that couldn't fit.
    let total_hidden = pending_hidden + ip_hidden;

    if total_hidden > 0 {
        // 判断隐藏的主要状态
        let status = if pending_hidden >= ip_hidden {
            "pending"
        } else {
            "in_progress"
        };
        lines.push(format_fold_hint(total_hidden, status));
    }

    lines
}

fn format_task_line(t: &Task) -> String {
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
    format!("{} #{} {}{}", icon, t.id, t.subject, owner)
}

fn format_fold_hint(n: usize, status: &str) -> String {
    format!("… +{} more {}", n, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task(id: &str, subject: &str, status: TaskStatus) -> Task {
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
            created_at: 0,
            updated_at: 0,
            session_id: None,
            tags: Vec::new(),
            batch: 0,
        }
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
        // summary + last completed + in_progress + 2 pending = 5
        assert_eq!(result.len(), 5);
        assert!(result[0].contains("2/5"));
        assert!(result[1].contains("✓ #2 done b")); // last completed
        assert!(result[2].contains("■ #3 doing c")); // in_progress
        assert!(result[3].contains("□ #4"));
        assert!(result[4].contains("□ #5"));
    }

    #[test]
    fn test_build_task_window_all_completed() {
        let tasks: Vec<_> = (1..=10)
            .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::Completed))
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
        // summary + 7 pending + fold (no completed/in_progress)
        assert_eq!(result.len(), 9);
        assert!(result[0].contains("0/20"));
        assert!(result.last().unwrap().contains("+13 more pending"));
    }

    #[test]
    fn test_build_task_window_in_progress_overflow() {
        let mut tasks: Vec<_> = (1..=10)
            .map(|i| make_task(&i.to_string(), &format!("task {}", i), TaskStatus::InProgress))
            .collect();
        tasks.push(make_task("11", "pending", TaskStatus::Pending));
        let result = build_task_window(&tasks, 7, 1);
        // summary + 7 in_progress (pending falls off) + fold
        assert_eq!(result.len(), 9);
        // pending should be hidden
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
}

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

    // --- completed: 按 task id 升序，保持任务列表稳定顺序 ---
    let all_completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut completed: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .collect();
    completed.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));
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
    pending.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));

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
        for t in completed.iter().skip(start) {
            lines.push(format_task_line(t));
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
        lines.push(format_task_line(t));
    }
    remaining = remaining.saturating_sub(comp_show);
    // 2. 所有 in_progress
    let ip_show = in_progress_count.min(remaining);
    for t in in_progress.iter().take(ip_show) {
        lines.push(format_task_line(t));
    }
    remaining = remaining.saturating_sub(ip_show);

    // 3. pending 填充余量
    let pending_show = pending_count.min(remaining);
    for t in pending.iter().take(pending_show) {
        lines.push(format_task_line(t));
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
            lines.insert(insert_pos + i, format_task_line(t));
        }
        remaining = remaining.saturating_sub(more_comp);
    }
    if remaining > 0 {
        // 补充更多 pending（跳过已显示的 pending_show 条）
        let more_pending = remaining.min(pending_count.saturating_sub(pending_show));
        for t in pending.iter().skip(pending_show).take(more_pending) {
            lines.push(format_task_line(t));
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
            lines.insert(1 + shown_completed + i, format_task_line(t));
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
#[path = "task_window_tests.rs"]
mod tests;

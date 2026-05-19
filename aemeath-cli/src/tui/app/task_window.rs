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

    // --- completed: 主窗口按 updated_at 降序取最近完成，额外扩展时再按 task id 稳定展示 ---
    let display_numbers = build_display_numbers(tasks);
    let all_completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut completed_by_recency: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .collect();
    completed_by_recency.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| task_sort_key(a).cmp(&task_sort_key(b)))
    });
    let in_progress: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InProgress)
        .collect();
    let mut pending: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    pending.sort_by_key(|t| task_sort_key(t));

    let total = tasks.len();
    let completed_count = all_completed_count;
    let in_progress_count = in_progress.len();
    let pending_count = pending.len();

    // TTL filter: only apply when there are more completed than max_lines.
    // If only completed + in_progress remain, keep enough completed context to fill the window.
    let newest_ts = completed_by_recency
        .iter()
        .map(|t| t.updated_at)
        .max()
        .unwrap_or(0);
    // 保存未过滤的 completed，用于温和扩展和下限保护回退
    let all_completed_for_fallback = completed_by_recency.clone();
    let completed_by_recency: Vec<&Task> = if completed_by_recency.len() > max_lines
        && !(pending_count == 0 && in_progress_count > 0)
    {
        completed_by_recency
            .into_iter()
            .filter(|t| newest_ts.saturating_sub(t.updated_at) <= COMPLETED_TTL_SECS)
            .collect()
    } else {
        completed_by_recency
    };
    let mut completed_for_display = completed_by_recency.clone();
    completed_for_display.sort_by_key(|t| task_sort_key(t));

    // 摘要行
    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    let capacity = max_lines;

    // 全部 completed 场景
    if in_progress_count == 0 && pending_count == 0 {
        let start = completed_for_display.len().saturating_sub(capacity);
        let shown = completed_for_display.len() - start;
        let hidden = completed_for_display.len() - shown;
        for t in completed_for_display.iter().skip(start) {
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
    let base_comp_show = show_last_completed
        .min(remaining)
        .min(completed_by_recency.len());
    let required_completed_fill = if pending_count == 0 {
        remaining
            .saturating_sub(in_progress_count)
            .min(completed_by_recency.len())
    } else {
        base_comp_show
    };
    let comp_show = base_comp_show.max(required_completed_fill);
    let selected_completed_ids: std::collections::HashSet<&str> = completed_by_recency
        .iter()
        .take(comp_show)
        .map(|task| task.id.as_str())
        .collect();
    let mut selected_completed: Vec<&Task> = completed_for_display
        .iter()
        .copied()
        .filter(|task| selected_completed_ids.contains(task.id.as_str()))
        .collect();
    selected_completed.sort_by_key(|task| task_sort_key(task));
    for t in selected_completed.iter() {
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
    // 构建按 id 升序的未过滤 completed 列表，用于回退补齐
    let mut all_completed_sorted = all_completed_for_fallback;
    all_completed_sorted.sort_by_key(|t| task_sort_key(t));
    // 跟踪已显示的 task id（避免重复插入）
    let mut shown_ids: std::collections::HashSet<&str> = selected_completed_ids;

    if remaining > 0 {
        // 先从 TTL 过滤后的 completed 中补充
        let from_filtered = remaining.min(completed_for_display.len().saturating_sub(comp_show));
        let extras: Vec<&Task> = completed_for_display
            .iter()
            .filter(|task| !shown_ids.contains(task.id.as_str()))
            .take(from_filtered)
            .copied()
            .collect();
        for t in extras.iter() {
            shown_ids.insert(t.id.as_str());
        }
        merge_completed_lines(&mut lines, 1, comp_show, &extras, &display_numbers);
        remaining = remaining.saturating_sub(extras.len());
    }
    if remaining > 0 {
        // 补充更多 pending
        let more_pending = remaining.min(pending_count.saturating_sub(pending_show));
        for t in pending.iter().skip(pending_show).take(more_pending) {
            lines.push(format_task_line(t, &display_numbers));
        }
        remaining = remaining.saturating_sub(more_pending);
    }
    // 如果仍有余量，从未过滤的 completed 中回退补齐
    if remaining > 0 {
        let fallback: Vec<&Task> = all_completed_sorted
            .iter()
            .filter(|t| !shown_ids.contains(t.id.as_str()))
            .take(remaining)
            .copied()
            .collect();
        for t in fallback.iter() {
            shown_ids.insert(t.id.as_str());
        }
        merge_completed_lines(&mut lines, 1, comp_show, &fallback, &display_numbers);
        remaining = remaining.saturating_sub(fallback.len());
    }
    // 如果仍然不足 min_show，继续从 completed 取
    if lines.len() - 1 < min_show && remaining > 0 {
        let need = (min_show - (lines.len() - 1)).min(remaining);
        let more: Vec<&Task> = all_completed_sorted
            .iter()
            .filter(|t| !shown_ids.contains(t.id.as_str()))
            .take(need)
            .copied()
            .collect();
        for t in more.iter() {
            shown_ids.insert(t.id.as_str());
        }
        merge_completed_lines(&mut lines, 1, comp_show, &more, &display_numbers);
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

fn task_sort_key(task: &Task) -> u64 {
    task.id.parse::<u64>().unwrap_or(u64::MAX)
}

fn merge_completed_lines(
    lines: &mut Vec<String>,
    start: usize,
    selected_count: usize,
    extra_tasks: &[&Task],
    display_numbers: &std::collections::HashMap<&str, usize>,
) {
    if extra_tasks.is_empty() {
        return;
    }

    let end = (start + selected_count).min(lines.len());
    let mut completed_lines: Vec<String> = lines.drain(start..end).collect();
    completed_lines.extend(
        extra_tasks
            .iter()
            .map(|task| format_task_line(task, display_numbers)),
    );
    completed_lines.sort_by_key(|line| display_line_number(line));
    for (offset, line) in completed_lines.into_iter().enumerate() {
        lines.insert(start + offset, line);
    }
}

fn display_line_number(line: &str) -> usize {
    line.split_once('#')
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .and_then(|number| number.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

fn build_display_numbers(tasks: &[Task]) -> std::collections::HashMap<&str, usize> {
    let mut ids: Vec<(&str, u64)> = tasks
        .iter()
        .map(|task| (task.id.as_str(), task_sort_key(task)))
        .collect();
    ids.sort_by_key(|(_, id)| *id);
    ids.into_iter()
        .enumerate()
        .map(|(index, (id, _))| (id, index + 1))
        .collect()
}

fn format_task_line(t: &Task, display_numbers: &std::collections::HashMap<&str, usize>) -> String {
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
    let display_id = display_numbers.get(t.id.as_str()).copied().unwrap_or(0);
    format!("{} #{} {}{}", icon, display_id, t.subject, owner)
}

fn format_fold_hint(n: usize, status: &str) -> String {
    format!("… +{} more {}", n, status)
}

#[cfg(test)]
#[path = "task_window_tests.rs"]
mod tests;

//! Task list 窗口化显示
//!
//! 排序策略：
//!   completed → 按 updated_at 升序；折叠窗口内固定显示最近完成的一条
//!   in_progress → 按 updated_at 升序（最早开始的在前）
//!   pending → 按 display_number 升序（稳定序）
//! 窗口化：不超过 max_lines 总行数时完整显示；超出时最近 completed + 尽量 in_progress + pending。

#[cfg(test)]
use sdk::{TaskState, TaskSummary};
#[cfg(test)]
use std::collections::HashMap;

/// 构建 task 状态显示行（窗口化，含摘要行）。
///
/// 规则：
/// 1. 摘要行 `━━ Tasks: completed/total ━━` 反映全量
/// 2. `summary + task 行数 <= max_lines` 时完整显示所有未删除任务，不折叠
/// 3. 超出 `max_lines` 时使用窗口策略：in_progress → pending → completed（从最新到最旧回填）
///    - completed 组内按 updated_at 升序，窗口内从最新往最旧回填剩余槽位
///    - in_progress 组内按 updated_at 升序（最早开始的在前）
///    - pending 组内按 display_number 升序
/// 4. 最多显示 `max_lines` 条总行数（含 summary 和 fold hint）
/// 5. 超出部分折叠提示 `… +N more`
/// 6. 空输入返回空 Vec
#[cfg(test)]
pub fn build_task_window(
    tasks: &[TaskSummary],
    display_map: &HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks
        .iter()
        .filter(|t| t.state != TaskState::Deleted)
        .count();
    let completed_count = tasks
        .iter()
        .filter(|t| t.state == TaskState::Completed)
        .count();

    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    let mut completed: Vec<&TaskSummary> = Vec::new();
    let mut in_progress: Vec<&TaskSummary> = Vec::new();
    let mut pending: Vec<&TaskSummary> = Vec::new();

    for t in tasks {
        match t.state {
            TaskState::Completed => completed.push(t),
            TaskState::InProgress => in_progress.push(t),
            TaskState::Pending => pending.push(t),
            TaskState::Deleted => {}
        }
    }

    // completed: 按完成时间升序，窗口内取最后一条作为“上一条 completed”
    completed.sort_by_key(|t| t.updated_at);
    // in_progress: 最早开始在前
    in_progress.sort_by_key(|t| t.updated_at);
    // pending: 按 display_number 稳定序
    sort_by_display_number(&mut pending, display_map);

    let task_slots_without_fold = max_lines.saturating_sub(1);
    let visible = if total <= task_slots_without_fold {
        ordered_tasks(completed, in_progress, pending)
    } else {
        let task_slots_with_fold = max_lines.saturating_sub(2);
        select_task_window(completed, in_progress, pending, task_slots_with_fold)
    };
    let shown_count = visible.len();
    let hidden_count = total.saturating_sub(shown_count);

    for t in visible {
        lines.push(format_task_line(t, display_map));
    }

    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }

    lines
}

#[cfg(test)]
fn ordered_tasks<'a>(
    completed: Vec<&'a TaskSummary>,
    in_progress: Vec<&'a TaskSummary>,
    pending: Vec<&'a TaskSummary>,
) -> Vec<&'a TaskSummary> {
    completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect()
}

#[cfg(test)]
fn select_task_window<'a>(
    completed: Vec<&'a TaskSummary>,
    in_progress: Vec<&'a TaskSummary>,
    pending: Vec<&'a TaskSummary>,
    max_lines: usize,
) -> Vec<&'a TaskSummary> {
    let mut visible = Vec::with_capacity(max_lines);
    if max_lines == 0 {
        return visible;
    }

    // Priority: in_progress → pending → completed (newest first backfill)
    visible.extend(in_progress.into_iter().take(max_lines));
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(pending.into_iter().take(remaining));
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(completed.iter().rev().take(remaining).copied());
    visible
}

#[cfg(test)]
fn sort_by_display_number(tasks: &mut [&TaskSummary], display_map: &HashMap<String, usize>) {
    tasks.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));
}

#[cfg(test)]
fn format_task_line(t: &TaskSummary, display_map: &HashMap<String, usize>) -> String {
    let icon = match t.state {
        TaskState::Completed => "✓",
        TaskState::InProgress => "■",
        TaskState::Pending => "□",
        TaskState::Deleted => "?",
    };
    let display_id = display_map.get(&t.id).copied().unwrap_or(0);
    let owner = t
        .owner
        .as_deref()
        .map(|o| format!(" (@{})", o))
        .unwrap_or_default();
    let blocked_by = format_blocked_by(&t.blocked_by, display_map);
    format!(
        "{} #{} {}{}{}",
        icon, display_id, t.subject, owner, blocked_by
    )
}

#[cfg(test)]
fn format_blocked_by(blocked_by: &[String], display_map: &HashMap<String, usize>) -> String {
    if blocked_by.is_empty() {
        return String::new();
    }

    let deps = blocked_by
        .iter()
        .map(|id| {
            display_map
                .get(id)
                .map(|display_id| format!("#{}", display_id))
                .unwrap_or_else(|| format!("#{}", id))
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" (blocked by {deps})")
}

#[cfg(test)]
#[path = "task_window_helpers_tests.rs"]
mod helpers_tests;
#[cfg(test)]
#[path = "task_window_progress_tests.rs"]
mod progress_tests;
#[cfg(test)]
#[path = "task_window_tests.rs"]
mod tests;

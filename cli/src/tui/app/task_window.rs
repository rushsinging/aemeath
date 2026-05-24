//! Task list 窗口化显示
//!
//! 排序策略：
//!   completed → 按 updated_at 降序（最近完成在前，反映实际执行顺序）
//!   in_progress → 按 updated_at 升序（最早开始的在前）
//!   pending → 按 display_number 升序（稳定序）
//! 窗口化：超出 max_lines 时截断并显示折叠提示

use aemeath_core::task::{Task, TaskStatus};
use std::collections::HashMap;

/// 构建 task 状态显示行（窗口化，含摘要行）。
///
/// 规则：
/// 1. 摘要行 `━━ Tasks: completed/total ━━` 反映全量
/// 2. 按状态分组排序：completed → in_progress → pending
///    - completed 组内按 updated_at 降序（最近完成在前）
///    - in_progress 组内按 updated_at 升序（最早开始的在前）
///    - pending 组内按 display_number 升序
/// 3. 最多显示 `max_lines` 条 task 行
/// 4. 超出部分折叠提示 `… +N more`
/// 5. 空输入返回空 Vec
pub fn build_task_window(
    tasks: &[Task],
    display_map: &HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();

    let summary = format!("━━ Tasks: {}/{} ━━", completed_count, total);
    let mut lines = vec![summary];

    let mut completed: Vec<&Task> = Vec::new();
    let mut in_progress: Vec<&Task> = Vec::new();
    let mut pending: Vec<&Task> = Vec::new();

    for t in tasks {
        match t.status {
            TaskStatus::Completed => completed.push(t),
            TaskStatus::InProgress => in_progress.push(t),
            TaskStatus::Pending => pending.push(t),
            _ => {}
        }
    }

    // completed: 最近完成在前
    completed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    // in_progress: 最早开始在前
    in_progress.sort_by_key(|t| t.updated_at);
    // pending: 按 display_number 稳定序
    sort_by_display_number(&mut pending, display_map);

    let ordered: Vec<&Task> = completed
        .into_iter()
        .chain(in_progress.into_iter())
        .chain(pending.into_iter())
        .collect();

    let shown_count = ordered.len().min(max_lines);
    let hidden_count = ordered.len() - shown_count;

    for t in ordered.iter().take(shown_count) {
        lines.push(format_task_line(t, display_map));
    }

    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }

    lines
}

fn sort_by_display_number(tasks: &mut [&Task], display_map: &HashMap<String, usize>) {
    tasks.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));
}

fn format_task_line(t: &Task, display_map: &HashMap<String, usize>) -> String {
    let icon = match t.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        _ => "?",
    };
    let display_id = display_map.get(&t.id).copied().unwrap_or(0);
    let owner = t
        .owner
        .as_deref()
        .map(|o| format!(" (@{})", o))
        .unwrap_or_default();
    format!("{} #{} {}{}", icon, display_id, t.subject, owner)
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

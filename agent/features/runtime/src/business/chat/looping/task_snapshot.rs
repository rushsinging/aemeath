//! Task snapshot 构造——纯渲染逻辑，从 `TaskStore` 取数据并渲染为
//! `TaskStatusView.lines`。
//!
//! 放在 business 层而非 core/client 层（COLA 分层：business 不可依赖 core，
//! core 可依赖 business；详见 `docs/design/02-architecture-guards.md`）。

use std::collections::HashMap;

use sdk::TaskStatusView;
use share::config::TaskListConfig;
use storage::api::{Task, TaskStatus};

/// 从 `task_store` 构造一份 `TaskStatusView` 快照（lines 已渲染好）。
///
/// 用于事件推送链路：runtime 在 task 生命周期关键点取快照，
/// 通过 `RuntimeStreamEvent::TasksSnapshot` 推送给前端，
/// 替代已被删除的 `changes()` 轮询路径（见 #567 / #642）。
pub(crate) async fn build_task_snapshot(task_store: &storage::api::TaskStore) -> TaskStatusView {
    let tasks = task_store.list_current_batch().await;
    let active: Vec<_> = tasks
        .iter()
        .filter(|t| t.status != TaskStatus::Deleted)
        .cloned()
        .collect();
    if active.is_empty() {
        return TaskStatusView::default();
    }

    let display_map = task_store.get_batch_display_map().await;
    let max_lines = TaskListConfig::default().max_lines;
    let lines = task_status_lines(&active, &display_map, max_lines);
    TaskStatusView { lines }
}

fn task_status_lines(
    tasks: &[Task],
    display_map: &HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks
        .iter()
        .filter(|t| t.status != TaskStatus::Deleted)
        .count();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&Task> = Vec::new();
    let mut in_progress: Vec<&Task> = Vec::new();
    let mut pending: Vec<&Task> = Vec::new();
    for task in tasks {
        match task.status {
            TaskStatus::Completed => completed.push(task),
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Pending => pending.push(task),
            TaskStatus::Deleted => {}
        }
    }
    completed.sort_by_key(|t| t.updated_at);
    in_progress.sort_by_key(|t| t.updated_at);
    pending.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));

    let visible = if total <= max_lines {
        ordered_tasks(completed, in_progress, pending)
    } else {
        select_task_window(completed, in_progress, pending, max_lines)
    };
    let shown_count = visible.len();
    let hidden_count = total.saturating_sub(shown_count);
    for task in visible {
        lines.push(format_task_status_line(task, display_map));
    }
    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }
    lines
}

fn ordered_tasks<'a>(
    completed: Vec<&'a Task>,
    in_progress: Vec<&'a Task>,
    pending: Vec<&'a Task>,
) -> Vec<&'a Task> {
    completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect()
}

fn select_task_window<'a>(
    completed: Vec<&'a Task>,
    in_progress: Vec<&'a Task>,
    pending: Vec<&'a Task>,
    max_lines: usize,
) -> Vec<&'a Task> {
    let mut visible = Vec::with_capacity(max_lines);
    if max_lines == 0 {
        return visible;
    }

    // Priority: completed (most recent N, ascending) → in_progress → pending
    // Reserve at least 1 slot for completed (if any exist)
    let mut completed_len = max_lines
        .saturating_sub(in_progress.len())
        .saturating_sub(pending.len());
    if !completed.is_empty() {
        completed_len = completed_len.max(1);
    }
    let skip = completed.len().saturating_sub(completed_len);
    visible.extend(completed.iter().skip(skip).take(completed_len).copied());
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(in_progress.into_iter().take(remaining));
    let remaining = max_lines.saturating_sub(visible.len());
    visible.extend(pending.into_iter().take(remaining));
    visible
}

fn format_task_status_line(task: &Task, display_map: &HashMap<String, usize>) -> String {
    let icon = match task.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let display_id = display_map.get(&task.id).copied().unwrap_or(0);
    let owner = task
        .owner
        .as_deref()
        .map(|owner| format!(" (@{})", owner))
        .unwrap_or_default();
    let blocked_by = format_blocked_by(&task.blocked_by, display_map);
    format!(
        "{} #{} {}{}{}",
        icon, display_id, task.subject, owner, blocked_by
    )
}

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

//! Task snapshot 构造——纯渲染逻辑，从 `TaskAccess` 取当前 batch 的
//! Task 投影并渲染为 `TaskStatusView.lines`。
//!
//! 放在 business 层而非 core/client 层（COLA 分层：business 不可依赖 core，
//! core 可依赖 business；详见 `docs/design/02-architecture-guards.md`）。

use std::collections::HashMap;

use sdk::TaskStatusView;
use share::config::TaskListConfig;
use task::{Task, TaskAccess, TaskId, TaskStatus};

/// 从 `TaskAccess` 构造一份 `TaskStatusView` 快照（lines 已渲染好）。
///
/// 用于事件推送链路：runtime 在 task 生命周期关键点取快照，
/// 通过 `RuntimeStreamEvent::TasksSnapshot` 推送给前端，
/// 替代已被删除的 `changes()` 轮询路径（见 #567 / #642）。
///
/// #889：改为同步 low-privilege 读取并按 current batch 过滤 Task PL。
/// 任务标题隐藏持久化 ID；依赖通过当前 batch 的局部编号显示。
pub(crate) fn build_task_snapshot(access: &dyn TaskAccess) -> TaskStatusView {
    let Some(current_batch) = access.reminder_snapshot().current_batch else {
        return TaskStatusView::default();
    };
    // `list()` 只返回 live（非 Deleted）Task；再按 current batch 收敛。
    let active: Vec<Task> = access
        .list()
        .into_iter()
        .filter(|task| task.batch() == current_batch)
        .collect();
    if active.is_empty() {
        return TaskStatusView::default();
    }

    let max_lines = TaskListConfig::default().max_lines;
    let lines = task_status_lines(&active, max_lines);
    TaskStatusView { lines }
}

fn task_status_lines(tasks: &[Task], max_lines: usize) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status() == TaskStatus::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&Task> = Vec::new();
    let mut in_progress: Vec<&Task> = Vec::new();
    let mut pending: Vec<&Task> = Vec::new();
    for task in tasks {
        match task.status() {
            TaskStatus::Completed => completed.push(task),
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Pending => pending.push(task),
            TaskStatus::Deleted => {}
        }
    }
    completed.sort_by_key(|t| t.updated_at());
    in_progress.sort_by_key(|t| t.updated_at());
    pending.sort_by_key(|t| t.id());

    let display_map = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id(), index + 1))
        .collect::<HashMap<_, _>>();
    let visible = if total <= max_lines {
        ordered_tasks(completed, in_progress, pending)
    } else {
        select_task_window(completed, in_progress, pending, max_lines)
    };
    let shown_count = visible.len();
    let hidden_count = total.saturating_sub(shown_count);
    for task in visible {
        lines.push(format_task_status_line(task, &display_map));
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

fn format_task_status_line(task: &Task, display_map: &HashMap<TaskId, usize>) -> String {
    let icon = match task.status() {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let blocked_by = format_blocked_by(task.blocked_by(), display_map);
    format!("{} {}{}", icon, task.subject(), blocked_by)
}

fn format_blocked_by(blocked_by: &[TaskId], display_map: &HashMap<TaskId, usize>) -> String {
    let deps = blocked_by
        .iter()
        .filter_map(|id| display_map.get(id))
        .map(|display_id| format!("#{display_id}"))
        .collect::<Vec<_>>();
    if deps.is_empty() {
        String::new()
    } else {
        format!(" (blocked by {})", deps.join(", "))
    }
}

#[cfg(test)]
#[path = "task_snapshot_tests.rs"]
mod tests;

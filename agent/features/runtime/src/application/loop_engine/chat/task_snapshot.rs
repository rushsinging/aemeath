//! Task state ACL：从 `TaskAccess` 构造结构化 SDK 状态，并为 LLM reminder
//! 保留独立的文本渲染出口。
//!
//! 放在 business 层而非 core/client 层（COLA 分层：business 不可依赖 core，
//! core 可依赖 business；详见 `docs/design/02-architecture-guards.md`）。

use std::collections::HashMap;

use sdk::{
    TaskBatchStatusView, TaskBatchView, TaskItemStatusView, TaskItemView, TaskPriorityView,
    TaskStateView,
};
use share::config::TaskListConfig;
use task::{BatchStatus, Task, TaskAccess, TaskId, TaskPriority, TaskStatus};

/// 从 `TaskAccess` 构造带 Session/revision 的完整结构化 Task state。
pub(crate) fn build_task_state_view(
    access: &dyn TaskAccess,
    session_id: impl Into<String>,
) -> TaskStateView {
    let session_id = session_id.into();
    let revision = access.revision().get();
    let Some(current_batch_id) = access.current_batch() else {
        return TaskStateView::empty(session_id, revision);
    };
    let Some(batch_snapshot) = access.batch_snapshot(current_batch_id) else {
        return TaskStateView::empty(session_id, revision);
    };
    let tasks = batch_snapshot.tasks();
    let total = tasks.len();
    let completed = tasks
        .iter()
        .filter(|task| task.status() == TaskStatus::Completed)
        .count();
    let in_progress = tasks
        .iter()
        .filter(|task| task.status() == TaskStatus::InProgress)
        .count();
    let mut completed_tasks: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatus::Completed)
        .collect();
    let mut in_progress_tasks: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatus::InProgress)
        .collect();
    let mut pending_tasks: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatus::Pending)
        .collect();
    completed_tasks.sort_by_key(|task| task.updated_at());
    in_progress_tasks.sort_by_key(|task| task.updated_at());
    pending_tasks.sort_by_key(|task| task.id());
    let max_items = TaskListConfig::default().max_lines;
    let visible_tasks = if tasks.len() <= max_items {
        ordered_tasks(completed_tasks, in_progress_tasks, pending_tasks)
    } else {
        select_task_window(completed_tasks, in_progress_tasks, pending_tasks, max_items)
    };
    let hidden_count = tasks.len().saturating_sub(visible_tasks.len());
    let sequence_by_id: HashMap<TaskId, u64> =
        tasks.iter().map(|task| (task.id(), task.seq())).collect();
    let items = visible_tasks
        .into_iter()
        .map(|task| TaskItemView {
            id: task.id().get(),
            sequence: task.seq(),
            subject: task.subject().to_owned(),
            status: match task.status() {
                TaskStatus::Pending => TaskItemStatusView::Pending,
                TaskStatus::InProgress => TaskItemStatusView::InProgress,
                TaskStatus::Completed => TaskItemStatusView::Completed,
                TaskStatus::Deleted => unreachable!("batch snapshot excludes deleted tasks"),
            },
            priority: match task.priority() {
                TaskPriority::Low => TaskPriorityView::Low,
                TaskPriority::Normal => TaskPriorityView::Normal,
                TaskPriority::High => TaskPriorityView::High,
                TaskPriority::Urgent => TaskPriorityView::Urgent,
            },
            blocked_by_sequences: task
                .blocked_by()
                .iter()
                .filter_map(|task_id| sequence_by_id.get(task_id).copied())
                .collect(),
        })
        .collect();
    let batch = batch_snapshot.batch();
    TaskStateView {
        session_id,
        revision,
        current_batch: Some(TaskBatchView {
            id: batch.id().get(),
            summary: batch.summary().map(str::to_owned),
            status: match batch.status() {
                BatchStatus::Active => TaskBatchStatusView::Active,
                BatchStatus::Paused => TaskBatchStatusView::Paused,
                BatchStatus::Archived => TaskBatchStatusView::Archived,
            },
        }),
        total,
        completed,
        in_progress,
        items,
        hidden_count,
    }
}

/// 当前 batch 的 live（非 Deleted）Task 列表；无 batch 或无任务时返回 `None`。
fn current_batch_tasks(access: &dyn TaskAccess) -> Option<Vec<Task>> {
    let current_batch = access.current_batch()?;
    let active: Vec<Task> = access
        .list()
        .into_iter()
        .filter(|task| task.batch() == current_batch)
        .collect();
    if active.is_empty() {
        None
    } else {
        Some(active)
    }
}

/// #1492：run 首步注入用——渲染 Task 进度为 `<system-reminder>` 文本，
/// 复用 `task_status_lines`（计数 + 分组排序 + max_lines 截断）。
///
/// 只出现在 invocation-only 的请求侧 messages；**NEVER** 写入 canonical
/// message、SDK/TUI 事件或持久化 JSON（spec 3.4.5 的显式例外）。
pub(crate) fn build_task_reminder(access: &dyn TaskAccess, max_lines: usize) -> Option<String> {
    let active = current_batch_tasks(access)?;
    let lines = task_status_lines(&active, max_lines);
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "<system-reminder>当前任务进度：\n{}\n</system-reminder>",
        lines.join("\n")
    ))
}

/// #1537：渲染当前 Task 状态为纯文本（无标签包装），供 compact summary 拼接。
///
/// 与 TUI/reminder 路径不同：compact summary 给 LLM 读，**MUST** 携带完整标识
///（batch id、task id、seq），使 LLM 能在压缩后精确引用 task。无活跃 batch
/// 或无任务时返回 `None`。
pub(crate) fn build_task_snapshot_text(access: &dyn TaskAccess) -> Option<String> {
    let batch_id = access.current_batch()?;
    let mut tasks: Vec<Task> = access
        .list()
        .into_iter()
        .filter(|task| task.batch() == batch_id && task.status() != TaskStatus::Deleted)
        .collect();
    if tasks.is_empty() {
        return None;
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status() == TaskStatus::Completed)
        .count();

    // 排序：Completed → InProgress → Pending，组内按 updated_at 升序。
    tasks.sort_by(|a, b| {
        let rank = |t: &Task| match t.status() {
            TaskStatus::Completed => 0,
            TaskStatus::InProgress => 1,
            TaskStatus::Pending => 2,
            TaskStatus::Deleted => 3,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.updated_at().cmp(&b.updated_at()))
    });

    let display_map: HashMap<TaskId, u64> =
        tasks.iter().map(|task| (task.id(), task.seq())).collect();

    let mut lines = vec![format!(
        "Batch #{batch_id} — Tasks: {completed_count}/{total}"
    )];
    for task in &tasks {
        lines.push(format_compact_task_line(task, &display_map));
    }
    Some(lines.join("\n"))
}

/// compact summary 专用渲染：携带完整标识（batch id / task id / seq）。
///
/// 与 TUI 的 `format_task_status_line`（隐藏持久化 ID）互补——compact 后
/// Agent 需要精确引用 task，标识不能丢失。
fn format_compact_task_line(task: &Task, display_map: &HashMap<TaskId, u64>) -> String {
    let icon = match task.status() {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let blocked_by = format_blocked_by(task.blocked_by(), display_map);
    format!(
        "{} [task:{} seq:{}] {}{}",
        icon,
        task.id().get(),
        task.seq(),
        task.subject(),
        blocked_by,
    )
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
        .map(|task| (task.id(), task.seq()))
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

fn format_task_status_line(task: &Task, display_map: &HashMap<TaskId, u64>) -> String {
    let icon = match task.status() {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let blocked_by = format_blocked_by(task.blocked_by(), display_map);
    format!("{} #{} {}{}", icon, task.seq(), task.subject(), blocked_by)
}

fn format_blocked_by(blocked_by: &[TaskId], display_map: &HashMap<TaskId, u64>) -> String {
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

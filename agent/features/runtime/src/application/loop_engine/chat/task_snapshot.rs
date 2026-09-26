//! TaskData state ACL：从 `TaskAccess` 构造结构化 SDK 状态，并为 LLM reminder
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
use task::{BatchStatusData, TaskAccess, TaskData, TaskIdData, TaskPriorityData, TaskStatusData};

/// 从 `TaskAccess` 构造带 Session/revision 的完整结构化 TaskData state。
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
        .filter(|task| task.status() == TaskStatusData::Completed)
        .count();
    let in_progress = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::InProgress)
        .count();
    let mut completed_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::Completed)
        .collect();
    let mut in_progress_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::InProgress)
        .collect();
    let mut pending_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::Pending)
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
    let sequence_by_id: HashMap<TaskIdData, u64> =
        tasks.iter().map(|task| (task.id(), task.seq())).collect();
    let items = visible_tasks
        .into_iter()
        .map(|task| TaskItemView {
            id: task.id().get(),
            sequence: task.seq(),
            subject: task.subject().to_owned(),
            status: match task.status() {
                TaskStatusData::Pending => TaskItemStatusView::Pending,
                TaskStatusData::InProgress => TaskItemStatusView::InProgress,
                TaskStatusData::Completed => TaskItemStatusView::Completed,
                TaskStatusData::Deleted => unreachable!("batch snapshot excludes deleted tasks"),
            },
            priority: match task.priority() {
                TaskPriorityData::Low => TaskPriorityView::Low,
                TaskPriorityData::Normal => TaskPriorityView::Normal,
                TaskPriorityData::High => TaskPriorityView::High,
                TaskPriorityData::Urgent => TaskPriorityView::Urgent,
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
                BatchStatusData::Active => TaskBatchStatusView::Active,
                BatchStatusData::Paused => TaskBatchStatusView::Paused,
                BatchStatusData::Archived => TaskBatchStatusView::Archived,
            },
        }),
        total,
        completed,
        in_progress,
        items,
        hidden_count,
    }
}

/// 当前 batch 的 live（非 Deleted）TaskData 列表；无 batch 或无任务时返回 `None`。
fn current_batch_tasks(access: &dyn TaskAccess) -> Option<Vec<TaskData>> {
    let current_batch = access.current_batch()?;
    let active: Vec<TaskData> = access
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

/// 将当前 TaskData 状态冻结为 Context-owned invocation reminder intent。
pub(crate) fn build_task_reminder_intent(
    access: &dyn TaskAccess,
    max_items: usize,
) -> Option<context::InvocationReminder> {
    let tasks = current_batch_tasks(access)?;
    if max_items == 0 {
        return None;
    }
    let total = tasks.len();
    let completed = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::Completed)
        .count();
    let mut completed_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::Completed)
        .collect();
    let mut in_progress_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::InProgress)
        .collect();
    let mut pending_tasks: Vec<&TaskData> = tasks
        .iter()
        .filter(|task| task.status() == TaskStatusData::Pending)
        .collect();
    completed_tasks.sort_by_key(|task| task.updated_at());
    in_progress_tasks.sort_by_key(|task| task.updated_at());
    pending_tasks.sort_by_key(|task| task.id());
    let visible = if total <= max_items {
        ordered_tasks(completed_tasks, in_progress_tasks, pending_tasks)
    } else {
        select_task_window(completed_tasks, in_progress_tasks, pending_tasks, max_items)
    };
    let sequence_by_id: HashMap<TaskIdData, u64> =
        tasks.iter().map(|task| (task.id(), task.seq())).collect();
    let items = visible
        .into_iter()
        .map(|task| context::TaskProgressReminderItem {
            sequence: task.seq(),
            subject: task.subject().to_owned(),
            status: match task.status() {
                TaskStatusData::Completed => context::TaskProgressStatus::Completed,
                TaskStatusData::InProgress => context::TaskProgressStatus::InProgress,
                TaskStatusData::Pending => context::TaskProgressStatus::Pending,
                TaskStatusData::Deleted => unreachable!("current batch excludes deleted tasks"),
            },
            blocked_by_sequences: task
                .blocked_by()
                .iter()
                .filter_map(|task_id| sequence_by_id.get(task_id).copied())
                .collect(),
        })
        .collect();
    let reminder = context::InvocationReminder::task_progress(context::TaskProgressReminder {
        total,
        completed,
        items,
        hidden_count: total.saturating_sub(max_items),
    });
    log::debug!(
        target: crate::LOG_TARGET,
        "invocation_reminder_created kind={} total={} completed={} visible={} hidden={}",
        reminder.kind(),
        total,
        completed,
        max_items.min(total),
        total.saturating_sub(max_items),
    );
    Some(reminder)
}

/// 将当前 TaskData aggregate 冻结为 Context-owned typed compact snapshot。
pub(crate) fn build_compact_task_snapshot(
    access: &dyn TaskAccess,
) -> Option<context::compact::CompactTaskSnapshot> {
    let batch_id = access.current_batch()?;
    let batch_snapshot = access.batch_snapshot(batch_id)?;
    let batch = batch_snapshot.batch();
    let batch_summary = batch.summary()?.trim();
    if batch_summary.is_empty() {
        return None;
    }
    let status = match batch.status() {
        BatchStatusData::Active => context::compact::CompactTaskBatchStatus::Active,
        BatchStatusData::Paused => context::compact::CompactTaskBatchStatus::Paused,
        BatchStatusData::Archived => context::compact::CompactTaskBatchStatus::Archived,
    };
    let sequence_by_id = batch_snapshot
        .tasks()
        .iter()
        .map(|task| (task.id(), task.seq()))
        .collect::<HashMap<_, _>>();
    let items = batch_snapshot
        .tasks()
        .iter()
        .filter(|task| task.status() != TaskStatusData::Deleted)
        .map(|task| {
            context::compact::CompactTaskItem::new(
                task.seq(),
                task.subject(),
                match task.status() {
                    TaskStatusData::Pending => context::compact::CompactTaskStatus::Pending,
                    TaskStatusData::InProgress => context::compact::CompactTaskStatus::InProgress,
                    TaskStatusData::Completed => context::compact::CompactTaskStatus::Completed,
                    TaskStatusData::Deleted => unreachable!("deleted tasks were filtered"),
                },
                task.blocked_by()
                    .iter()
                    .filter_map(|task_id| sequence_by_id.get(task_id).copied())
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    Some(context::compact::CompactTaskSnapshot::new(
        access.revision().get(),
        batch.id().get(),
        batch_summary,
        status,
        items,
    ))
}

#[cfg(test)]
fn task_status_lines(tasks: &[TaskData], max_lines: usize) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status() == TaskStatusData::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&TaskData> = Vec::new();
    let mut in_progress: Vec<&TaskData> = Vec::new();
    let mut pending: Vec<&TaskData> = Vec::new();
    for task in tasks {
        match task.status() {
            TaskStatusData::Completed => completed.push(task),
            TaskStatusData::InProgress => in_progress.push(task),
            TaskStatusData::Pending => pending.push(task),
            TaskStatusData::Deleted => {}
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
    completed: Vec<&'a TaskData>,
    in_progress: Vec<&'a TaskData>,
    pending: Vec<&'a TaskData>,
) -> Vec<&'a TaskData> {
    completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect()
}

fn select_task_window<'a>(
    completed: Vec<&'a TaskData>,
    in_progress: Vec<&'a TaskData>,
    pending: Vec<&'a TaskData>,
    max_lines: usize,
) -> Vec<&'a TaskData> {
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

#[cfg(test)]
fn format_task_status_line(task: &TaskData, display_map: &HashMap<TaskIdData, u64>) -> String {
    let icon = match task.status() {
        TaskStatusData::Completed => "✓",
        TaskStatusData::InProgress => "■",
        TaskStatusData::Pending => "□",
        TaskStatusData::Deleted => "?",
    };
    let blocked_by = format_blocked_by(task.blocked_by(), display_map);
    format!("{} #{} {}{}", icon, task.seq(), task.subject(), blocked_by)
}

#[cfg(test)]
fn format_blocked_by(blocked_by: &[TaskIdData], display_map: &HashMap<TaskIdData, u64>) -> String {
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

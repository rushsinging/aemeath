use crate::domain::types::task_update::{
    TaskProgressItemResult, TaskProgressLifecycleResult, TaskProgressListResult,
    TaskProgressOmittedResult, TaskProgressResult, TaskUpdateInput, TaskUpdateResult,
};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskId, TaskPriority, TaskStatus};

pub struct TaskUpdateTool {
    pub access: Arc<dyn TaskAccess>,
}

fn parse_seq(value: &str, field: &str) -> Result<u64, String> {
    TaskId::parse_tool_input(value)
        .map(TaskId::get)
        .map_err(|_| format!("{field} must be a non-zero decimal task ID sequence: {value}"))
}

fn task_id(
    access: &dyn TaskAccess,
    task_list_id: Option<&str>,
    value: &str,
    field: &str,
) -> Result<TaskId, String> {
    let seq = parse_seq(value, field)?;
    match task_list_id {
        Some(value) => {
            let batch_id = value
                .parse::<u64>()
                .ok()
                .filter(|id| *id > 0)
                .map(task::BatchId::new)
                .ok_or_else(|| format!("invalid task list id: {value}"))?;
            access
                .batch_snapshot(batch_id)
                .and_then(|snapshot| {
                    snapshot
                        .tasks()
                        .iter()
                        .find(|task| task.seq() == seq)
                        .map(task::Task::id)
                })
                .ok_or_else(|| format!("Task not found in task list #{batch_id}: {value}"))
        }
        None => access
            .current_task_by_seq(seq)
            .map(|task| task.id())
            .ok_or_else(|| format!("Task not found in current task list: {value}")),
    }
}

fn parse_priority(value: &str) -> Result<TaskPriority, String> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Ok(TaskPriority::Low),
        "normal" | "medium" => Ok(TaskPriority::Normal),
        "high" => Ok(TaskPriority::High),
        "urgent" | "critical" => Ok(TaskPriority::Urgent),
        _ => Err(format!("invalid priority: {value}")),
    }
}

fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Completed => "completed",
        TaskStatus::Deleted => "deleted",
    }
}

fn display_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "Pending",
        TaskStatus::InProgress => "InProgress",
        TaskStatus::Completed => "Completed",
        TaskStatus::Deleted => "Deleted",
    }
}

fn task_list_status_label(status: task::BatchStatus) -> &'static str {
    match status {
        task::BatchStatus::Active => "active",
        task::BatchStatus::Paused => "paused",
        task::BatchStatus::Archived => "archived",
    }
}

fn progress_item(item: &task::TaskProgressItem) -> TaskProgressItemResult {
    TaskProgressItemResult {
        task_id: item.seq.to_string(),
        subject: item.subject.clone(),
        status: status_label(item.status).to_owned(),
    }
}

fn progress_result(snapshot: &task::TaskProgressSnapshot) -> TaskProgressResult {
    TaskProgressResult {
        task_list: TaskProgressListResult {
            task_list_id: snapshot.task_list_id.to_string(),
            summary: snapshot.summary.clone().unwrap_or_default(),
            status: task_list_status_label(snapshot.task_list_status).to_owned(),
        },
        updated: progress_item(&snapshot.updated),
        recently_completed: snapshot
            .recently_completed
            .iter()
            .map(progress_item)
            .collect(),
        in_progress: snapshot.in_progress.iter().map(progress_item).collect(),
        ready: snapshot.ready.iter().map(progress_item).collect(),
        omitted: TaskProgressOmittedResult {
            ready: snapshot.ready_omitted,
            blocked: snapshot.blocked_count,
        },
        lifecycle: TaskProgressLifecycleResult {
            auto_closed: snapshot.auto_closed,
            auto_reopened: snapshot.auto_reopened,
        },
    }
}

fn render_progress(snapshot: &task::TaskProgressSnapshot, lang: &str) -> String {
    let mut lines = vec![if lang == "zh" {
        format!(
            "任务列表 #{}「{}」当前进度：",
            snapshot.task_list_id,
            snapshot.summary.as_deref().unwrap_or("未命名")
        )
    } else {
        format!(
            "Current task list #{} \"{}\":",
            snapshot.task_list_id,
            snapshot.summary.as_deref().unwrap_or("Untitled")
        )
    }];
    if !snapshot.recently_completed.is_empty() {
        lines.push(
            if lang == "zh" {
                "最近完成："
            } else {
                "Recently completed:"
            }
            .to_owned(),
        );
        lines.extend(
            snapshot
                .recently_completed
                .iter()
                .map(|item| format!("- #{} {}", item.seq, item.subject)),
        );
    }
    if !snapshot.in_progress.is_empty() {
        lines.push(
            if lang == "zh" {
                "进行中："
            } else {
                "In progress:"
            }
            .to_owned(),
        );
        lines.extend(
            snapshot
                .in_progress
                .iter()
                .map(|item| format!("- #{} {}", item.seq, item.subject)),
        );
    }
    if !snapshot.ready.is_empty() {
        lines.push(
            if lang == "zh" {
                "可执行："
            } else {
                "Ready:"
            }
            .to_owned(),
        );
        lines.extend(
            snapshot
                .ready
                .iter()
                .map(|item| format!("- #{} {}", item.seq, item.subject)),
        );
    }
    if snapshot.ready_omitted > 0 || snapshot.blocked_count > 0 {
        lines.push(if lang == "zh" {
            format!(
                "另有 {} 个可执行、{} 个被阻塞任务未展开。",
                snapshot.ready_omitted, snapshot.blocked_count
            )
        } else {
            format!(
                "{} additional ready and {} blocked tasks are omitted.",
                snapshot.ready_omitted, snapshot.blocked_count
            )
        });
    }
    if snapshot.auto_closed {
        lines.push(if lang == "zh" {
            "所有任务均已完成，任务列表已自动关闭，后续不会再提醒。".to_owned()
        } else {
            "All tasks are complete. The task list was automatically closed and will not generate further reminders.".to_owned()
        });
    } else if snapshot.auto_reopened {
        lines.push(if lang == "zh" {
            "该任务列表已自动重新打开。".to_owned()
        } else {
            "The task list was automatically reopened.".to_owned()
        });
    }
    lines.join("\n")
}

fn priority_label(priority: TaskPriority) -> &'static str {
    match priority {
        TaskPriority::Low => "low",
        TaskPriority::Normal => "normal",
        TaskPriority::High => "high",
        TaskPriority::Urgent => "urgent",
    }
}

#[async_trait]
impl TypedTool for TaskUpdateTool {
    type Output = TaskUpdateResult;
    fn name(&self) -> &str {
        "TaskUpdate"
    }
    fn description(&self) -> &str {
        "Update a single field on a task. Valid keys: status, subject, description, priority."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_update(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskUpdateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskUpdateResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskUpdateResult> {
        let args: TaskUpdateInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let id = match task_id(
            self.access.as_ref(),
            args.task_list_id.as_deref(),
            &args.task_id,
            "task_id",
        ) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };
        let value = match args.value.as_str() {
            Some(value) => value,
            None => {
                return TypedToolResult::error(format!(
                    "value must be a string for key '{}'",
                    args.key
                ))
            }
        };
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        if args.key == "status" {
            let target = match value {
                "pending" => TaskStatus::Pending,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                "deleted" => TaskStatus::Deleted,
                _ => return TypedToolResult::error(format!("invalid status: {value}")),
            };
            let snapshot = if target == TaskStatus::Deleted {
                self.access.delete_with_progress(id, timestamp)
            } else {
                self.access.transition_with_progress(id, target, timestamp)
            };
            let snapshot = match snapshot {
                Ok(result) => result.value,
                Err(error) => return TypedToolResult::error(error.to_string()),
            };
            let updated = self
                .access
                .get(id)
                .expect("updated task must remain readable");
            let task_id = updated.seq().to_string();
            let status = status_label(updated.status()).to_owned();
            let text = format!(
                "Task #{} updated. Status: {}\n\n{}",
                task_id,
                display_status(updated.status()),
                render_progress(&snapshot, ctx.guidance().language())
            );
            return TypedToolResult::success(
                text,
                TaskUpdateResult {
                    task_id,
                    status,
                    subject: updated.subject().to_owned(),
                    priority: priority_label(updated.priority()).to_owned(),
                    blocked_by: updated
                        .blocked_by()
                        .iter()
                        .filter_map(|id| {
                            self.access.get(*id).map(|task| format!("#{}", task.seq()))
                        })
                        .collect(),
                    progress: Some(progress_result(&snapshot)),
                },
            );
        }
        let result = match args.key.as_str() {
            "subject" => self.access.set_subject(id, value.to_owned(), timestamp),
            "description" => self.access.set_description(id, value.to_owned(), timestamp),
            "priority" => {
                let priority = match parse_priority(value) {
                    Ok(priority) => priority,
                    Err(error) => return TypedToolResult::error(error),
                };
                self.access.set_priority(id, priority, timestamp)
            }
            // `owner` and dependency fields are intentionally rejected: they are not
            // mutable through TaskUpdate's Published Language.
            key => {
                return TypedToolResult::error(format!(
                    "unknown field '{key}'. Valid keys: status, subject, description, priority"
                ))
            }
        };
        let updated = match result {
            Ok(result) => result.value,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let task_id = updated.seq().to_string();
        let status = status_label(updated.status()).to_owned();
        let blocked_by = updated
            .blocked_by()
            .iter()
            .filter_map(|id| self.access.get(*id).map(|task| format!("#{}", task.seq())))
            .collect();
        TypedToolResult::success(
            format!("Task #{} updated. Status: {}", task_id, status),
            TaskUpdateResult {
                task_id,
                status,
                subject: updated.subject().to_owned(),
                priority: priority_label(updated.priority()).to_owned(),
                blocked_by,
                progress: None,
            },
        )
    }
}

#[cfg(test)]
#[path = "task_update_tests.rs"]
mod tests;

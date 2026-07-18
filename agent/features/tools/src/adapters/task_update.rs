use crate::domain::types::task_update::{TaskUpdateInput, TaskUpdateResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskId, TaskPriority, TaskStatus};

pub struct TaskUpdateTool {
    pub access: Arc<dyn TaskAccess>,
}

fn parse_id(value: &str, field: &str) -> Result<TaskId, String> {
    value
        .parse::<u64>()
        .map(TaskId::new)
        .map_err(|_| format!("{field} must be a decimal task ID: {value}"))
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
        TaskStatus::Pending => "Pending",
        TaskStatus::InProgress => "InProgress",
        TaskStatus::Completed => "Completed",
        TaskStatus::Deleted => "Deleted",
    }
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
        "Update a single field on a task. Valid keys: status, subject, description, priority, blocked_by_id."
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
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskUpdateResult> {
        let args: TaskUpdateInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let id = match parse_id(&args.task_id, "task_id") {
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
        let result = match args.key.as_str() {
            "status" => match value {
                "pending" => self.access.transition(id, TaskStatus::Pending, timestamp),
                "in_progress" => self.access.transition(id, TaskStatus::InProgress, timestamp),
                // Task BC supports Pending -> Completed as one atomic transition/commit.
                "completed" => self.access.transition(id, TaskStatus::Completed, timestamp),
                "deleted" => self.access.delete(id, timestamp),
                _ => return TypedToolResult::error(format!("invalid status: {value}")),
            },
            "subject" => self.access.set_subject(id, value.to_owned(), timestamp),
            "description" => self.access.set_description(id, value.to_owned(), timestamp),
            "priority" => {
                let priority = match parse_priority(value) {
                    Ok(priority) => priority,
                    Err(error) => return TypedToolResult::error(error),
                };
                self.access.set_priority(id, priority, timestamp)
            }
            "blocked_by_id" => {
                let dependency = match parse_id(value, "blocked_by_id") {
                    Ok(id) => id,
                    Err(error) => return TypedToolResult::error(error),
                };
                self.access.add_dependency(id, dependency, timestamp)
            }
            // `owner` is intentionally rejected: it is not in Task's Published Language.
            key => return TypedToolResult::error(format!(
                "unknown field '{key}'. Valid keys: status, subject, description, priority, blocked_by_id"
            )),
        };
        let updated = match result {
            Ok(result) => result.value,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let task_id = updated.id().to_string();
        let status = status_label(updated.status()).to_owned();
        TypedToolResult::success(
            format!("Task #{} updated. Status: {}", task_id, status),
            TaskUpdateResult {
                task_id,
                status,
                subject: updated.subject().to_owned(),
                priority: priority_label(updated.priority()).to_owned(),
                blocked_by: updated
                    .blocked_by()
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect(),
            },
        )
    }
}

#[cfg(test)]
#[path = "task_update_tests.rs"]
mod tests;

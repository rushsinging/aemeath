use crate::domain::types::task_list::{TaskListInput, TaskListResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskPriority, TaskStatus};

pub struct TaskListTool {
    pub access: Arc<dyn TaskAccess>,
}

fn parse_priority(value: &str) -> Option<TaskPriority> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Some(TaskPriority::Low),
        "normal" | "medium" => Some(TaskPriority::Normal),
        "high" => Some(TaskPriority::High),
        "urgent" | "critical" => Some(TaskPriority::Urgent),
        _ => None,
    }
}

/// Temporary anti-corruption mapping while tool results retain the legacy wire DTO.
fn to_legacy_task(task: &task::Task) -> share::task::types::Task {
    use share::task::types::{TaskPriority as LegacyPriority, TaskStatus as LegacyStatus};
    share::task::types::Task {
        id: task.id().to_string(),
        subject: task.subject().to_owned(),
        description: task.description().to_owned(),
        status: match task.status() {
            TaskStatus::Pending => LegacyStatus::Pending,
            TaskStatus::InProgress => LegacyStatus::InProgress,
            TaskStatus::Completed => LegacyStatus::Completed,
            TaskStatus::Deleted => LegacyStatus::Deleted,
        },
        owner: None,
        blocked_by: task.blocked_by().iter().map(ToString::to_string).collect(),
        priority: match task.priority() {
            TaskPriority::Low => LegacyPriority::Low,
            TaskPriority::Normal => LegacyPriority::Normal,
            TaskPriority::High => LegacyPriority::High,
            TaskPriority::Urgent => LegacyPriority::Urgent,
        },
        created_at: task.created_at(),
        updated_at: task.updated_at(),
        session_id: task.session_id().map(str::to_owned),
        batch: task.batch().get(),
    }
}

#[async_trait]
impl TypedTool for TaskListTool {
    type Output = TaskListResult;
    fn name(&self) -> &str {
        "TaskList"
    }
    fn description(&self) -> &str {
        "List all tasks and their status. Use to discover pending work with no unresolved dependencies."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_list(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskListResult> {
        let args: TaskListInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let status = args.status.as_deref().and_then(|value| match value {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "completed" => Some(TaskStatus::Completed),
            "deleted" => Some(TaskStatus::Deleted),
            _ => None,
        });
        let priority = args.priority.as_deref().and_then(parse_priority);
        let mut tasks = self.access.list();
        if let Some(status) = status {
            tasks.retain(|task| task.status() == status);
        }
        if let Some(priority) = priority {
            tasks.retain(|task| task.priority() == priority);
        }
        let stats = self.access.stats();
        let message = if tasks.is_empty() {
            "No tasks found".to_owned()
        } else {
            format!(
                "{} tasks ({} pending, {} in_progress, {} completed)",
                stats.total - stats.deleted,
                stats.pending,
                stats.in_progress,
                stats.completed
            )
        };
        TypedToolResult::success(
            message,
            TaskListResult {
                tasks: tasks.iter().map(to_legacy_task).collect(),
            },
        )
    }
}

use crate::domain::types::task_list::{TaskListInput, TaskListResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskPriority, TaskStatus, TaskView};

pub struct TaskListTool {
    pub access: Arc<dyn TaskAccess>,
}

#[cfg(test)]
#[path = "task_list_tests.rs"]
mod tests;

fn parse_priority(value: &str) -> Option<TaskPriority> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Some(TaskPriority::Low),
        "normal" | "medium" => Some(TaskPriority::Normal),
        "high" => Some(TaskPriority::High),
        "urgent" | "critical" => Some(TaskPriority::Urgent),
        _ => None,
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
        let current_batch = self.access.current_batch();
        let all_tasks = self.access.list();
        let seq_by_id = all_tasks
            .iter()
            .filter(|task| Some(task.batch()) == current_batch)
            .map(|task| (task.id(), task.seq().to_string()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut tasks: Vec<_> = all_tasks
            .into_iter()
            .filter(|task| Some(task.batch()) == current_batch)
            .collect();
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
                tasks: tasks
                    .iter()
                    .map(|task| {
                        TaskView::from_task(
                            task,
                            task.blocked_by()
                                .iter()
                                .filter_map(|id| seq_by_id.get(id).cloned())
                                .collect(),
                        )
                    })
                    .collect(),
            },
        )
    }
}

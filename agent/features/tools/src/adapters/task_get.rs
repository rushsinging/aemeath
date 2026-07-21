use crate::domain::types::task_get::{TaskGetInput, TaskGetResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskId, TaskView};

pub struct TaskGetTool {
    pub access: Arc<dyn TaskAccess>,
}

#[cfg(test)]
#[path = "task_get_tests.rs"]
mod tests;

fn parse_task_id(value: &str) -> Result<TaskId, String> {
    TaskId::parse_tool_input(value)
        .map_err(|_| format!("Task ID must be a non-zero decimal number: {value}"))
}

#[async_trait]
impl TypedTool for TaskGetTool {
    type Output = TaskGetResult;
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn description(&self) -> &str {
        "Retrieve a task by ID. Returns task details including subject, description, status, and dependencies."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_get(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskGetInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskGetResult::data_schema()
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
    ) -> TypedToolResult<TaskGetResult> {
        let args: TaskGetInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let id = match parse_task_id(&args.task_id) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };
        let task = match self.access.get(id) {
            Some(task) if task.status() != task::TaskStatus::Deleted => task,
            _ => return TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        };
        TypedToolResult::success(
            format!("Task #{}: {}", task.id(), task.subject()),
            TaskGetResult {
                task: TaskView::from(&task),
            },
        )
    }
}

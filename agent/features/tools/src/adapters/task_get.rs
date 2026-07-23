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

fn current_task(access: &dyn TaskAccess, value: &str) -> Result<task::Task, String> {
    let seq = TaskId::parse_tool_input(value)
        .map(TaskId::get)
        .map_err(|_| format!("Task ID must be a non-zero decimal number: {value}"))?;
    access
        .current_task_by_seq(seq)
        .ok_or_else(|| format!("Task not found: {value}"))
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
        let task = match current_task(self.access.as_ref(), &args.task_id) {
            Ok(task) => task,
            Err(error) => return TypedToolResult::error(error),
        };
        let blocked_by = task
            .blocked_by()
            .iter()
            .filter_map(|id| self.access.get(*id).map(|task| task.seq().to_string()))
            .collect();
        TypedToolResult::success(
            format!("Task #{}: {}", task.seq(), task.subject()),
            TaskGetResult {
                task: TaskView::from_task(&task, blocked_by),
            },
        )
    }
}

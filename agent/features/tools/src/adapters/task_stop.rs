use crate::domain::types::task_stop::{TaskStopInput, TaskStopResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskId, TaskStatus};

pub struct TaskStopTool {
    pub access: Arc<dyn TaskAccess>,
}

#[cfg(test)]
#[path = "task_stop_tests.rs"]
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
impl TypedTool for TaskStopTool {
    type Output = TaskStopResult;
    fn name(&self) -> &str {
        "TaskStop"
    }
    fn description(&self) -> &str {
        "Stop a running or pending task. Marks the task as deleted and cancels any associated work."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_stop(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskStopInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskStopResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskStopResult> {
        let args: TaskStopInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let task = match current_task(self.access.as_ref(), &args.task_id) {
            Ok(task) => task,
            Err(error) => return TypedToolResult::error(error),
        };
        let id = task.id();
        match task.status() {
            TaskStatus::Completed => {
                return TypedToolResult::error(format!(
                    "Task #{} is already completed and cannot be stopped",
                    task.seq()
                ))
            }
            TaskStatus::Deleted => {
                return TypedToolResult::error(format!("Task #{} is already deleted", task.seq()))
            }
            _ => {}
        }
        if let Err(error) = self
            .access
            .delete(id, chrono::Utc::now().timestamp_millis() as u64)
        {
            return TypedToolResult::error(error.to_string());
        }
        TypedToolResult::success(
            format!("Task #{} stopped and marked as deleted", task.seq()),
            TaskStopResult {
                task_id: task.seq().to_string(),
            },
        )
    }
}

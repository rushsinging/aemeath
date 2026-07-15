use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_stop::{TaskStopInput, TaskStopResult};
use std::sync::Arc;
use storage::{TaskStatus, TaskStore};

pub struct TaskStopTool {
    pub store: Arc<TaskStore>,
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
        use share::tool::types::ToolSchema;
        TaskStopInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskStopResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        // Mutates persistent task state; keep ordered with related task operations.
        false
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskStopResult> {
        let args: TaskStopInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let input_id = args.task_id.as_str();

        if input_id.is_empty() {
            return TypedToolResult::error("Task ID is required");
        }

        // Resolve display number to global id
        let task_id = match self.store.resolve_display_id(input_id).await {
            Some(global_id) => global_id,
            None => return TypedToolResult::error(format!("Task not found: {}", input_id)),
        };
        let display_id = self.store.format_display_id(&task_id).await;

        let task = self.store.get(&task_id).await;

        if task.is_none() {
            return TypedToolResult::error(format!("Task not found: {}", display_id));
        }
        let task = task.unwrap();

        // Check if task can be stopped
        match task.status {
            TaskStatus::Completed => {
                return TypedToolResult::error(format!(
                    "Task #{} is already completed and cannot be stopped",
                    display_id
                ));
            }
            TaskStatus::Deleted => {
                return TypedToolResult::error(format!("Task #{} is already deleted", display_id));
            }
            _ => {}
        }

        // Mark task as deleted
        self.store
            .update(&task_id, |t| {
                t.status = TaskStatus::Deleted;
            })
            .await;

        TypedToolResult::success(
            format!("Task #{} stopped and marked as deleted", display_id),
            TaskStopResult {
                task_id: display_id,
            },
        )
    }
}

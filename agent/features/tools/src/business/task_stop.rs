use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_stop::TaskStopResult;
use std::sync::Arc;
use storage::api::{TaskStatus, TaskStore};

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
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The ID of the task to stop"
                }
            },
            "required": ["taskId"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskStopResult> {
        let input_id = input["taskId"].as_str().unwrap_or("");

        if input_id.is_empty() {
            return TypedToolResult::error_value(serde_json::json!({
                "status": "error",
                "message": "Task ID is required",
                "data": {}
            }));
        }

        // Resolve display number to global id
        let task_id = match self.store.resolve_display_id(input_id).await {
            Some(global_id) => global_id,
            None => {
                return TypedToolResult::error_value(serde_json::json!({
                    "status": "error",
                    "message": format!("Task not found: {}", input_id),
                    "data": {}
                }))
            }
        };
        let display_id = self.store.format_display_id(&task_id).await;

        let task = self.store.get(&task_id).await;

        if task.is_none() {
            return TypedToolResult::error_value(serde_json::json!({
                "status": "error",
                "message": format!("Task not found: {}", display_id),
                "data": {}
            }));
        }
        let task = task.unwrap();

        // Check if task can be stopped
        match task.status {
            TaskStatus::Completed => {
                return TypedToolResult::error_value(serde_json::json!({
                    "status": "error",
                    "message": format!("Task #{} is already completed and cannot be stopped", display_id),
                    "data": { "task_id": display_id, "status": "completed" }
                }));
            }
            TaskStatus::Deleted => {
                return TypedToolResult::error_value(serde_json::json!({
                    "status": "error",
                    "message": format!("Task #{} is already deleted", display_id),
                    "data": { "task_id": display_id, "status": "deleted" }
                }));
            }
            _ => {}
        }

        // Mark task as deleted
        self.store
            .update(&task_id, |t| {
                t.status = TaskStatus::Deleted;
            })
            .await;

        TypedToolResult::success_value(serde_json::json!({
            "status": "success",
            "message": format!("Task #{} stopped and marked as deleted", display_id),
            "data": serde_json::to_value(TaskStopResult { task_id: display_id }).unwrap()
        }))
    }
}

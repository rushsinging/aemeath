use async_trait::async_trait;
use kernel::task::TaskStore;
use kernel::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::sync::Arc;

pub struct TaskStopTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskStopTool {
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

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let task_id = input["taskId"].as_str().unwrap_or("");

        if task_id.is_empty() {
            return ToolResult::error("Task ID is required");
        }

        let task = self.store.get(task_id).await;

        if task.is_none() {
            return ToolResult::error(format!("Task not found: {}", task_id));
        }
        let task = task.unwrap();

        // Check if task can be stopped
        match task.status {
            kernel::task::TaskStatus::Completed => {
                return ToolResult::error(format!(
                    "Task #{} is already completed and cannot be stopped",
                    task_id
                ));
            }
            kernel::task::TaskStatus::Deleted => {
                return ToolResult::error(format!("Task #{} is already deleted", task_id));
            }
            _ => {}
        }

        // Mark task as deleted
        self.store
            .update(task_id, |t| {
                t.status = kernel::task::TaskStatus::Deleted;
            })
            .await;

        ToolResult::success(format!("Task #{} stopped and marked as deleted", task_id))
    }
}

use async_trait::async_trait;
use serde_json::Value;
use share::tool::{Tool, ToolContext, ToolResult};
use std::sync::Arc;
use storage::api::TaskStore;

/// TaskOutputTool manages task outputs and results.
/// Provides access to task execution results and output history.
pub struct TaskOutputTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }
    fn description(&self) -> &str {
        "Get task output and results. Use this to retrieve the output from completed or in-progress tasks."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task ID to get output for"
                },
                "all": {
                    "type": "boolean",
                    "description": "Get output for all tasks (default: false)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of outputs to return (default: 10)",
                    "default": 10
                }
            },
            "required": []
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let all = input["all"].as_bool().unwrap_or(false);
        let limit = input["limit"].as_i64().unwrap_or(10) as usize;

        if all {
            // Get all task outputs
            let tasks = self.store.list().await;
            if tasks.is_empty() {
                return ToolResult::success("No tasks found");
            }

            let mut output = String::new();
            let count = tasks.len().min(limit);

            for task in tasks.iter().take(count) {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = match task.status {
                    storage::api::TaskStatus::Pending => "pending",
                    storage::api::TaskStatus::InProgress => "in_progress",
                    storage::api::TaskStatus::Completed => "completed",
                    storage::api::TaskStatus::Deleted => "deleted",
                };

                output.push_str(&format!("#{} [{}] {}\n", display_id, status, task.subject));
                output.push_str(&format!("  Description: {}\n", task.description));

                if let Some(owner) = &task.owner {
                    output.push_str(&format!("  Owner: {}\n", owner));
                }

                if !task.blocked_by.is_empty() {
                    let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
                    output.push_str(&format!("  Blocked by: {}\n", dep_displays.join(", ")));
                }

                if !task.blocks.is_empty() {
                    let dep_displays = self.store.to_display_ids(&task.blocks).await;
                    output.push_str(&format!("  Blocks: {}\n", dep_displays.join(", ")));
                }

                output.push('\n');
            }

            if tasks.len() > limit {
                output.push_str(&format!(
                    "\n... and {} more tasks (use limit parameter to see more)",
                    tasks.len() - limit
                ));
            }

            ToolResult::success(output.trim_end())
        } else if let Some(input_id) = input["task_id"].as_str() {
            // Resolve display number to global id
            let task_id = match self.store.resolve_display_id(input_id).await {
                Some(global_id) => global_id,
                None => return ToolResult::error(format!("Task not found: {}", input_id)),
            };
            // Get specific task output
            match self.store.get(&task_id).await {
                Some(task) => {
                    let display_id = self.store.format_display_id(&task.id).await;
                    let status = match task.status {
                        storage::api::TaskStatus::Pending => "pending",
                        storage::api::TaskStatus::InProgress => "in_progress",
                        storage::api::TaskStatus::Completed => "completed",
                        storage::api::TaskStatus::Deleted => "deleted",
                    };

                    let mut output = String::new();
                    output.push_str(&format!("Task #{} [{}]\n", display_id, status));
                    output.push_str(&format!("Subject: {}\n", task.subject));
                    output.push_str(&format!("Description: {}\n", task.description));

                    if let Some(owner) = &task.owner {
                        output.push_str(&format!("Owner: {}\n", owner));
                    }

                    if let Some(active_form) = &task.active_form {
                        output.push_str(&format!("Active Form: {}\n", active_form));
                    }

                    if !task.blocked_by.is_empty() {
                        let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
                        output.push_str(&format!("Blocked by: {}\n", dep_displays.join(", ")));
                    }

                    if !task.blocks.is_empty() {
                        let dep_displays = self.store.to_display_ids(&task.blocks).await;
                        output.push_str(&format!("Blocks: {}\n", dep_displays.join(", ")));
                    }

                    ToolResult::success(output.trim_end())
                }
                None => ToolResult::error(format!("Task not found: {}", input_id)),
            }
        } else {
            // No task_id specified, show recent tasks
            let tasks = self.store.list().await;
            if tasks.is_empty() {
                return ToolResult::success("No tasks found. Use TaskCreate to create a task.");
            }

            let mut output =
                String::from("Recent tasks (use task_id parameter to get details):\n\n");
            let count = tasks.len().min(limit);

            for task in tasks.iter().take(count) {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = match task.status {
                    storage::api::TaskStatus::Pending => "pending",
                    storage::api::TaskStatus::InProgress => "in_progress",
                    storage::api::TaskStatus::Completed => "completed",
                    storage::api::TaskStatus::Deleted => "deleted",
                };

                output.push_str(&format!("#{} [{}] {}\n", display_id, status, task.subject));
            }

            if tasks.len() > limit {
                output.push_str(&format!("\n... and {} more tasks", tasks.len() - limit));
            }

            ToolResult::success(output.trim_end())
        }
    }
}

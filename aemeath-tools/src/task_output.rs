use aemeath_core::task::TaskStore;
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// TaskOutputTool manages task outputs and results.
/// Provides access to task execution results and output history.
pub struct TaskOutputTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str { "TaskOutput" }
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
    fn is_read_only(&self) -> bool { true }
    fn is_concurrency_safe(&self) -> bool { true }

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
                let status = match task.status {
                    aemeath_core::task::TaskStatus::Pending => "pending",
                    aemeath_core::task::TaskStatus::InProgress => "in_progress",
                    aemeath_core::task::TaskStatus::Completed => "completed",
                    aemeath_core::task::TaskStatus::Deleted => "deleted",
                };
                
                output.push_str(&format!(
                    "#{} [{}] {}\n",
                    task.id, status, task.subject
                ));
                output.push_str(&format!("  Description: {}\n", task.description));
                
                if let Some(owner) = &task.owner {
                    output.push_str(&format!("  Owner: {}\n", owner));
                }
                
                if !task.blocked_by.is_empty() {
                    output.push_str(&format!("  Blocked by: {}\n", task.blocked_by.join(", ")));
                }
                
                if !task.blocks.is_empty() {
                    output.push_str(&format!("  Blocks: {}\n", task.blocks.join(", ")));
                }
                
                output.push('\n');
            }

            if tasks.len() > limit {
                output.push_str(&format!("\n... and {} more tasks (use limit parameter to see more)", tasks.len() - limit));
            }

            ToolResult::success(output.trim_end())
        } else if let Some(task_id) = input["task_id"].as_str() {
            // Get specific task output
            match self.store.get(task_id).await {
                Some(task) => {
                    let status = match task.status {
                        aemeath_core::task::TaskStatus::Pending => "pending",
                        aemeath_core::task::TaskStatus::InProgress => "in_progress",
                        aemeath_core::task::TaskStatus::Completed => "completed",
                        aemeath_core::task::TaskStatus::Deleted => "deleted",
                    };

                    let mut output = String::new();
                    output.push_str(&format!("Task #{} [{}]\n", task.id, status));
                    output.push_str(&format!("Subject: {}\n", task.subject));
                    output.push_str(&format!("Description: {}\n", task.description));
                    
                    if let Some(owner) = &task.owner {
                        output.push_str(&format!("Owner: {}\n", owner));
                    }
                    
                    if let Some(active_form) = &task.active_form {
                        output.push_str(&format!("Active Form: {}\n", active_form));
                    }
                    
                    if !task.blocked_by.is_empty() {
                        output.push_str(&format!("Blocked by: {}\n", task.blocked_by.join(", ")));
                    }
                    
                    if !task.blocks.is_empty() {
                        output.push_str(&format!("Blocks: {}\n", task.blocks.join(", ")));
                    }

                    ToolResult::success(output.trim_end())
                }
                None => ToolResult::error(format!("Task not found: {}", task_id)),
            }
        } else {
            // No task_id specified, show recent tasks
            let tasks = self.store.list().await;
            if tasks.is_empty() {
                return ToolResult::success("No tasks found. Use TaskCreate to create a task.");
            }

            let mut output = String::from("Recent tasks (use task_id parameter to get details):\n\n");
            let count = tasks.len().min(limit);
            
            for task in tasks.iter().take(count) {
                let status = match task.status {
                    aemeath_core::task::TaskStatus::Pending => "pending",
                    aemeath_core::task::TaskStatus::InProgress => "in_progress",
                    aemeath_core::task::TaskStatus::Completed => "completed",
                    aemeath_core::task::TaskStatus::Deleted => "deleted",
                };
                
                output.push_str(&format!(
                    "#{} [{}] {}\n",
                    task.id, status, task.subject
                ));
            }

            if tasks.len() > limit {
                output.push_str(&format!("\n... and {} more tasks", tasks.len() - limit));
            }

            ToolResult::success(output.trim_end())
        }
    }
}

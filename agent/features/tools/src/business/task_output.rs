use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
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

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let all = input["all"].as_bool().unwrap_or(false);
        let limit = input["limit"].as_i64().unwrap_or(10) as usize;

        if all {
            // Get all task outputs
            let tasks = self.store.list().await;
            if tasks.is_empty() {
                return ToolResult::success_json(serde_json::json!({
                    "status": "success",
                    "message": "No tasks found",
                    "data": { "tasks": [] }
                }));
            }

            let mut tasks_json = serde_json::json!([]);
            let count = tasks.len().min(limit);

            for task in tasks.iter().take(count) {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = match task.status {
                    storage::api::TaskStatus::Pending => "pending",
                    storage::api::TaskStatus::InProgress => "in_progress",
                    storage::api::TaskStatus::Completed => "completed",
                    storage::api::TaskStatus::Deleted => "deleted",
                };

                let mut task_obj = serde_json::json!({
                    "id": display_id,
                    "status": status,
                    "subject": task.subject,
                    "description": task.description
                });

                if let Some(ref owner) = task.owner {
                    task_obj["owner"] = serde_json::Value::String(owner.clone());
                }

                if !task.blocked_by.is_empty() {
                    let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
                    task_obj["blocked_by"] = serde_json::json!(dep_displays);
                }

                if !task.blocks.is_empty() {
                    let dep_displays = self.store.to_display_ids(&task.blocks).await;
                    task_obj["blocks"] = serde_json::json!(dep_displays);
                }

                tasks_json.as_array_mut().unwrap().push(task_obj);
            }

            let has_more = tasks.len() > limit;
            let remaining = if has_more { tasks.len() - limit } else { 0 };

            ToolResult::success_json(serde_json::json!({
                "status": "success",
                "message": format!("{} tasks found", count),
                "data": {
                    "tasks": tasks_json,
                    "total": tasks.len(),
                    "returned": count,
                    "limit": limit,
                    "has_more": has_more,
                    "remaining": remaining
                }
            }))
        } else if let Some(input_id) = input["task_id"].as_str() {
            // Resolve display number to global id
            let task_id = match self.store.resolve_display_id(input_id).await {
                Some(global_id) => global_id,
                None => {
                    return ToolResult::error_json(serde_json::json!({
                        "status": "error",
                        "message": format!("Task not found: {}", input_id),
                        "data": {}
                    }))
                }
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

                    let mut data = serde_json::json!({
                        "id": display_id,
                        "subject": task.subject,
                        "status": status,
                        "description": task.description
                    });

                    if let Some(ref owner) = task.owner {
                        data["owner"] = serde_json::Value::String(owner.clone());
                    }

                    if let Some(ref active_form) = task.active_form {
                        data["active_form"] = serde_json::Value::String(active_form.clone());
                    }

                    if !task.blocked_by.is_empty() {
                        let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
                        data["blocked_by"] = serde_json::json!(dep_displays);
                    }

                    if !task.blocks.is_empty() {
                        let dep_displays = self.store.to_display_ids(&task.blocks).await;
                        data["blocks"] = serde_json::json!(dep_displays);
                    }

                    ToolResult::success_json(serde_json::json!({
                        "status": "success",
                        "message": format!("Task #{}: {}", display_id, task.subject),
                        "data": data
                    }))
                }
                None => ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("Task not found: {}", input_id),
                    "data": {}
                })),
            }
        } else {
            // No task_id specified, show recent tasks
            let tasks = self.store.list().await;
            if tasks.is_empty() {
                return ToolResult::success_json(serde_json::json!({
                    "status": "success",
                    "message": "No tasks found. Use TaskCreate to create a task.",
                    "data": { "tasks": [] }
                }));
            }

            let mut tasks_json = serde_json::json!([]);
            let count = tasks.len().min(limit);

            for task in tasks.iter().take(count) {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = match task.status {
                    storage::api::TaskStatus::Pending => "pending",
                    storage::api::TaskStatus::InProgress => "in_progress",
                    storage::api::TaskStatus::Completed => "completed",
                    storage::api::TaskStatus::Deleted => "deleted",
                };

                tasks_json.as_array_mut().unwrap().push(serde_json::json!({
                    "id": display_id,
                    "status": status,
                    "subject": task.subject
                }));
            }

            let has_more = tasks.len() > limit;
            let remaining = if has_more { tasks.len() - limit } else { 0 };

            ToolResult::success_json(serde_json::json!({
                "status": "success",
                "message": format!("{} recent tasks (use task_id for details)", count),
                "data": {
                    "tasks": tasks_json,
                    "total": tasks.len(),
                    "returned": count,
                    "has_more": has_more,
                    "remaining": remaining
                }
            }))
        }
    }
}

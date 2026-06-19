use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_get::TaskGetResult;
use std::sync::Arc;
use storage::api::{TaskStatus, TaskStore};

pub struct TaskGetTool {
    pub store: Arc<TaskStore>,
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
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The ID of the task to retrieve"
                }
            },
            "required": ["taskId"]
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskGetResult> {
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
                }));
            }
        };

        let task = match self.store.get(&task_id).await {
            Some(t) => t,
            None => {
                return TypedToolResult::error_value(serde_json::json!({
                    "status": "error",
                    "message": format!("Task not found: {}", input_id),
                    "data": {}
                }));
            }
        };

        let display_id = self.store.format_display_id(&task.id).await;
        let status = match task.status {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Deleted => "deleted",
        };

        let priority = task.priority.as_str();

        let mut task_data = serde_json::json!({
            "id": display_id,
            "subject": task.subject.clone(),
            "status": status,
            "priority": priority,
            "description": task.description,
            "progress": task.progress,
            "created_at": format_timestamp(task.created_at),
            "updated_at": format_timestamp(task.updated_at),
        });

        if let Some(ref progress_message) = task.progress_message {
            task_data["progress_message"] = serde_json::Value::String(progress_message.clone());
        }
        if let Some(ref owner) = task.owner {
            task_data["owner"] = serde_json::Value::String(owner.clone());
        }
        if let Some(ref active_form) = task.active_form {
            task_data["active_form"] = serde_json::Value::String(active_form.clone());
        }
        if let Some(ref session_id) = task.session_id {
            task_data["session_id"] = serde_json::Value::String(session_id.clone());
        }
        if !task.tags.is_empty() {
            task_data["tags"] = serde_json::json!(task.tags);
        }

        // Dependencies
        if !task.blocked_by.is_empty() {
            let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
            let blocked_by: Vec<String> =
                dep_displays.iter().map(|id| format!("#{}", id)).collect();
            let is_blocked = self.store.is_blocked(&task).await;
            task_data["blocked_by"] = serde_json::json!(blocked_by);
            task_data["is_blocked"] = serde_json::json!(is_blocked);
        }

        if !task.blocks.is_empty() {
            let dep_displays = self.store.to_display_ids(&task.blocks).await;
            let blocks: Vec<String> = dep_displays.iter().map(|id| format!("#{}", id)).collect();
            task_data["blocks"] = serde_json::json!(blocks);
        }

        TypedToolResult::success_value(serde_json::json!({
            "status": "success",
            "message": format!("Task #{}: {}", display_id, task.subject),
            "data": {
                "task": task_data
            }
        }))
    }
}

/// Format timestamp as human-readable string
fn format_timestamp(ts: u64) -> String {
    // Simple format: convert to ISO-like string
    let secs = ts / 1000;
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let s = rem % 60;

    // Approximate date from 1970
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if d < days_in_year {
            break;
        }
        d -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for days_in_month in &month_days {
        if d < *days_in_month as i64 {
            break;
        }
        d -= *days_in_month as i64;
        m += 1;
    }

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y,
        m + 1,
        d + 1,
        hours,
        mins,
        s
    )
}

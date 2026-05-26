use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::task_ops::{TaskStatus, TaskStore};
use std::sync::Arc;

pub struct TaskGetTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskGetTool {
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

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let input_id = input["taskId"].as_str().unwrap_or("");

        if input_id.is_empty() {
            return ToolResult::error("Task ID is required");
        }

        // Resolve display number to global id
        let task_id = match self.store.resolve_display_id(input_id).await {
            Some(global_id) => global_id,
            None => return ToolResult::error(format!("Task not found: {}", input_id)),
        };

        let task = match self.store.get(&task_id).await {
            Some(t) => t,
            None => return ToolResult::error(format!("Task not found: {}", input_id)),
        };

        let display_id = self.store.format_display_id(&task.id).await;
        let status = match task.status {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Deleted => "deleted",
        };

        let priority = task.priority.as_str();

        let mut lines = vec![
            format!("Task #{}: {}", display_id, task.subject),
            format!("Status: {}", status),
            format!("Priority: {}", priority),
            format!("Description: {}", task.description),
        ];

        // Progress
        if task.progress > 0 {
            let progress_str = format!("Progress: {}%", task.progress);
            let msg_str = task
                .progress_message
                .as_ref()
                .map(|m| format!(" - {}", m))
                .unwrap_or_default();
            lines.push(format!("{}{}", progress_str, msg_str));
        }

        // Owner
        if let Some(owner) = &task.owner {
            lines.push(format!("Owner: {}", owner));
        }

        // Active form
        if let Some(active_form) = &task.active_form {
            lines.push(format!("Active form: {}", active_form));
        }

        // Session
        if let Some(session_id) = &task.session_id {
            lines.push(format!("Session: {}", session_id));
        }

        // Tags
        if !task.tags.is_empty() {
            lines.push(format!("Tags: {}", task.tags.join(", ")));
        }

        // Dependencies
        if !task.blocked_by.is_empty() {
            let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
            let blocked_by = dep_displays
                .iter()
                .map(|id| format!("#{}", id))
                .collect::<Vec<_>>()
                .join(", ");
            let blocked_status = if task.is_blocked(&self.store).await {
                " (currently blocked)"
            } else {
                ""
            };
            lines.push(format!("Blocked by: {}{}", blocked_by, blocked_status));
        }

        if !task.blocks.is_empty() {
            let dep_displays = self.store.to_display_ids(&task.blocks).await;
            let blocks = dep_displays
                .iter()
                .map(|id| format!("#{}", id))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("Blocks: {}", blocks));
        }

        // Timestamps
        lines.push(format!("Created: {}", format_timestamp(task.created_at)));
        lines.push(format!("Updated: {}", format_timestamp(task.updated_at)));

        ToolResult::success(lines.join("\n"))
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

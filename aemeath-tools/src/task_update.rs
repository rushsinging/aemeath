use aemeath_core::task::{TaskPriority, TaskStatus, TaskStore};
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct TaskUpdateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str { "TaskUpdate" }
    fn description(&self) -> &str {
        "Update a task's status, subject, description, or dependencies.\n\nStatus workflow: pending → in_progress → completed. Use 'deleted' to remove."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string", "description": "The ID of the task to update" },
                "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"] },
                "subject": { "type": "string" },
                "description": { "type": "string" },
                "activeForm": { "type": "string" },
                "owner": { "type": "string" },
                "priority": { "type": "string", "enum": ["low", "normal", "high", "urgent"] },
                "progress": { "type": "integer", "minimum": 0, "maximum": 100, "description": "Progress percentage (0-100)" },
                "progressMessage": { "type": "string", "description": "Progress status message" },
                "addBlockedBy": { "type": "array", "items": { "type": "string" } },
                "addBlocks": { "type": "array", "items": { "type": "string" } },
                "addTags": { "type": "array", "items": { "type": "string" } },
                "removeTags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["taskId"]
        })
    }
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let task_id = match input.get("taskId").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return ToolResult::error("missing required parameter: taskId"),
        };

        let result = self.store.update(&task_id, |task| {
            // Status update
            if let Some(status) = input.get("status").and_then(|v| v.as_str()) {
                task.status = match status {
                    "pending" => TaskStatus::Pending,
                    "in_progress" => TaskStatus::InProgress,
                    "completed" => TaskStatus::Completed,
                    "deleted" => TaskStatus::Deleted,
                    _ => task.status.clone(),
                };
            }

            // Basic field updates
            if let Some(subject) = input.get("subject").and_then(|v| v.as_str()) {
                task.subject = subject.to_string();
            }
            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                task.description = desc.to_string();
            }
            if let Some(af) = input.get("activeForm").and_then(|v| v.as_str()) {
                task.active_form = Some(af.to_string());
            }
            if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
                task.owner = Some(owner.to_string());
            }

            // Priority update
            if let Some(priority) = input.get("priority").and_then(|v| v.as_str()) {
                if let Some(p) = TaskPriority::from_str(priority) {
                    task.priority = p;
                }
            }

            // Progress update
            if let Some(progress) = input.get("progress").and_then(|v| v.as_u64()) {
                task.progress = (progress as u8).min(100);
            }
            if let Some(msg) = input.get("progressMessage").and_then(|v| v.as_str()) {
                task.progress_message = Some(msg.to_string());
            }

            // Dependency updates
            if let Some(blocked_by) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
                for id in blocked_by {
                    if let Some(s) = id.as_str() {
                        if !task.blocked_by.contains(&s.to_string()) {
                            task.blocked_by.push(s.to_string());
                        }
                    }
                }
            }
            if let Some(blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
                for id in blocks {
                    if let Some(s) = id.as_str() {
                        if !task.blocks.contains(&s.to_string()) {
                            task.blocks.push(s.to_string());
                        }
                    }
                }
            }

            // Tag updates
            if let Some(add_tags) = input.get("addTags").and_then(|v| v.as_array()) {
                for tag in add_tags {
                    if let Some(t) = tag.as_str() {
                        task.add_tag(t.to_string());
                    }
                }
            }
            if let Some(remove_tags) = input.get("removeTags").and_then(|v| v.as_array()) {
                for tag in remove_tags {
                    if let Some(t) = tag.as_str() {
                        task.remove_tag(t);
                    }
                }
            }
        }).await;


        match result {
            Some(task) => {
                let progress_str = if task.progress > 0 {
                    format!(" ({}%{})", task.progress,
                        task.progress_message.as_ref()
                            .map(|m| format!(" - {}", m))
                            .unwrap_or_default())
                } else {
                    "".to_string()
                };
                ToolResult::success(format!(
                    "Updated task #{}: {} [{}]{}\nStatus: {:?}",
                    task.id, task.subject, task.priority.as_str(), progress_str, task.status
                ))
            }
            None => ToolResult::error(format!("task not found: {task_id}")),
        }
    }
}

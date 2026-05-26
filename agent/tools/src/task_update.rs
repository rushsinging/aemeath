use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::task_ops::{TaskPriority, TaskStatus, TaskStore};
use std::sync::Arc;

pub struct TaskUpdateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "TaskUpdate"
    }
    fn description(&self) -> &str {
        "Update a task's status, subject, description, or dependencies.\n\n\
         Status workflow: pending → in_progress → completed. Use 'deleted' to remove.\n\n\
         When you mark a task as completed, the system will show which downstream tasks \
         are now unblocked and ready to execute. Use this to decide what to work on next.\n\n\
         After completing a task, check the unblocked list or call TaskList to find the next available task."
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
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let input_id = match input.get("taskId").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return ToolResult::error("missing required parameter: taskId"),
        };

        // Resolve display number (batch-local id) to global task id
        let task_id = match self.store.resolve_display_id(&input_id).await {
            Some(global_id) => global_id,
            None => return ToolResult::error(format!("task not found: {input_id}")),
        };

        // Pre-resolve dependency display numbers to global ids (must be async)
        let resolved_blocked_by =
            if let Some(arr) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
                let display_ids: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                self.store.resolve_display_ids(&display_ids).await
            } else {
                Vec::new()
            };
        let resolved_blocks = if let Some(arr) = input.get("addBlocks").and_then(|v| v.as_array()) {
            let display_ids: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            self.store.resolve_display_ids(&display_ids).await
        } else {
            Vec::new()
        };

        let result = self
            .store
            .update(&task_id, |task| {
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

                // Dependency updates — use pre-resolved global ids
                for gid in &resolved_blocked_by {
                    if !task.blocked_by.contains(gid) {
                        task.blocked_by.push(gid.clone());
                    }
                }
                for gid in &resolved_blocks {
                    if !task.blocks.contains(gid) {
                        task.blocks.push(gid.clone());
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
            })
            .await;

        match result {
            Some(task) => {
                let display_id = self.store.format_display_id(&task.id).await;

                let progress_str = if task.progress > 0 {
                    format!(
                        " ({}%{})",
                        task.progress,
                        task.progress_message
                            .as_ref()
                            .map(|m| format!(" - {}", m))
                            .unwrap_or_default()
                    )
                } else {
                    "".to_string()
                };
                let mut output = format!(
                    "Updated task #{}: {} [{}]{}\nStatus: {:?}",
                    display_id,
                    task.subject,
                    task.priority.as_str(),
                    progress_str,
                    task.status
                );

                // When a task is completed, show which downstream tasks are now unblocked
                if task.status == TaskStatus::Completed {
                    let all_tasks = self.store.list().await;
                    // Collect all completed task IDs for dependency resolution
                    let completed_ids: std::collections::HashSet<&str> = all_tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::Completed)
                        .map(|t| t.id.as_str())
                        .collect();

                    let newly_unblocked: Vec<_> = all_tasks
                        .iter()
                        .filter(|t| {
                            t.status == TaskStatus::Pending
                                && !t.blocked_by.is_empty()
                                && t.blocked_by.iter().any(|dep| dep == &task_id)
                                && t.blocked_by
                                    .iter()
                                    .all(|dep| completed_ids.contains(dep.as_str()))
                        })
                        .collect();

                    if !newly_unblocked.is_empty() {
                        output.push_str("\n\nUnblocked tasks now ready:");
                        for t in &newly_unblocked {
                            let t_display = self.store.format_display_id(&t.id).await;
                            let dep_displays = self.store.to_display_ids(&t.blocked_by).await;
                            let deps = dep_displays
                                .iter()
                                .map(|d| format!("#{d}"))
                                .collect::<Vec<_>>()
                                .join(", ");
                            output.push_str(&format!(
                                "\n  → #{} \"{}\" (was blocked by {})",
                                t_display, t.subject, deps
                            ));
                        }
                    }

                    // Also show remaining pending tasks count
                    let remaining_pending = all_tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::Pending)
                        .count();
                    if remaining_pending > 0 {
                        output
                            .push_str(&format!("\n\n{} task(s) still pending.", remaining_pending));
                    } else {
                        output.push_str("\n\nAll tasks completed!");
                    }
                }

                ToolResult::success(output)
            }
            None => ToolResult::error(format!("task not found: {input_id}")),
        }
    }
}

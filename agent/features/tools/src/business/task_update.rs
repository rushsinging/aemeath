use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use storage::api::{TaskPriority, TaskStatus, TaskStore};

fn current_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or_default()
}

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

    async fn call(&self, input: serde_json::Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let now = current_timestamp_millis();
        let input_id = match input.get("taskId").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": "missing required parameter: taskId",
                    "data": {}
                }))
            }
        };

        // Resolve display number (batch-local id) to global task id
        let task_id = match self.store.resolve_display_id(&input_id).await {
            Some(global_id) => global_id,
            None => {
                return ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": format!("task not found: {input_id}"),
                    "data": { "task_id": input_id }
                }))
            }
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
                    if let Some(p) = TaskPriority::parse(priority) {
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
                            task.add_tag(t.to_string(), now);
                        }
                    }
                }
                if let Some(remove_tags) = input.get("removeTags").and_then(|v| v.as_array()) {
                    for tag in remove_tags {
                        if let Some(t) = tag.as_str() {
                            task.remove_tag(t, now);
                        }
                    }
                }
            })
            .await;

        match result {
            Some(task) => {
                let display_id = self.store.format_display_id(&task.id).await;

                let mut data = serde_json::json!({
                    "task_id": display_id,
                    "subject": task.subject,
                    "status": format!("{:?}", task.status),
                    "priority": task.priority.as_str(),
                    "progress": task.progress,
                });

                if let Some(ref pm) = task.progress_message {
                    data["progress_message"] = serde_json::Value::String(pm.clone());
                }
                if let Some(ref af) = task.active_form {
                    data["active_form"] = serde_json::Value::String(af.clone());
                }
                if let Some(ref owner) = task.owner {
                    data["owner"] = serde_json::Value::String(owner.clone());
                }

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

                    let mut unblocked_list = Vec::new();
                    for t in &newly_unblocked {
                        let t_display = self.store.format_display_id(&t.id).await;
                        let dep_displays = self.store.to_display_ids(&t.blocked_by).await;
                        let deps: Vec<String> =
                            dep_displays.iter().map(|d| format!("#{d}")).collect();
                        unblocked_list.push(serde_json::json!({
                            "task_id": t_display,
                            "subject": t.subject,
                            "blocked_by": deps
                        }));
                    }
                    data["unblocked_tasks"] = serde_json::json!(unblocked_list);

                    let remaining_pending = all_tasks
                        .iter()
                        .filter(|t| t.status == TaskStatus::Pending)
                        .count();
                    data["remaining_pending"] = serde_json::json!(remaining_pending);
                }

                let message = format!("Task #{} updated", display_id);
                ToolResult::success_json(serde_json::json!({
                    "status": "success",
                    "message": message,
                    "data": data
                }))
            }
            None => ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": format!("task not found: {input_id}"),
                "data": { "task_id": input_id }
            })),
        }
    }
}

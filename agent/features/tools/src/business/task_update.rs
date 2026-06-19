use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_update::TaskUpdateResult;
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
impl TypedTool for TaskUpdateTool {
    type Output = TaskUpdateResult;
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
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskUpdateResult::data_schema()
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
    ) -> TypedToolResult<TaskUpdateResult> {
        let now = current_timestamp_millis();
        let input_id = match input.get("taskId").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return TypedToolResult::error_value(serde_json::json!({
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
                return TypedToolResult::error_value(serde_json::json!({
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
                let status = format!("{:?}", task.status);

                let message = format!("Task #{} updated", display_id);
                TypedToolResult::success_value(serde_json::json!({
                    "status": "success",
                    "message": message,
                    "data": serde_json::to_value(TaskUpdateResult { task_id: display_id, status }).unwrap()
                }))
            }
            None => TypedToolResult::error_value(serde_json::json!({
                "status": "error",
                "message": format!("task not found: {input_id}"),
                "data": { "task_id": input_id }
            })),
        }
    }
}

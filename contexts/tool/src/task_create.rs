use async_trait::async_trait;
use kernel::task::{TaskPriority, TaskStore};
use kernel::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::sync::Arc;

pub struct TaskCreateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn description(&self) -> &str {
        "Create a task to track progress on complex multi-step work only.\n\n\
           Use task management only when the user request requires at least 3 substantial execution steps,\n\
           multiple dependent changes, or parallel sub-agent coordination. Do NOT create tasks for simple\n\
           one-step requests such as answering a question, inspecting a file, checking bug status, running a\n\
           single command, or making a tiny localized edit. For simple requests, execute directly.\n\n\
           IMPORTANT workflow when task management is actually needed:\n\
           1. First, describe your complete plan as text — list ALL planned tasks so the user can see the full picture\n\
           2. For a new complex multi-step user request, call TaskListCreate before TaskCreate so tasks attach to a request summary\n\
           3. Then create tasks one by one with TaskCreate\n\
           4. Use TaskUpdate to set dependencies and assign agents\n\n\
           After creating tasks, use TaskUpdate to:\n\
           - Set dependencies (addBlockedBy/addBlocks) between tasks\n\
           - Mark as in_progress before starting work\n\
           - Mark as completed when done — the system will show which tasks are unblocked\n\n\
           Use TaskList to discover pending tasks with no unresolved dependencies.\n\
           Launch Agent for independent tasks that can run in parallel."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "description": "A brief title for the task" },
                "description": { "type": "string", "description": "What needs to be done" },
                "activeForm": { "type": "string", "description": "Present continuous form for spinner display" },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high", "urgent"],
                    "description": "Task priority level"
                },
                "sessionId": { "type": "string", "description": "Session ID to associate with this task" },
                "owner": { "type": "string", "description": "Task owner" },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for categorization"
                }
            },
            "required": ["subject", "description"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let subject = match input.get("subject").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error("missing required parameter: subject"),
        };
        let description = match input.get("description").and_then(|v| v.as_str()) {
            Some(d) => d.to_string(),
            None => return ToolResult::error("missing required parameter: description"),
        };
        let active_form = input
            .get("activeForm")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Parse priority
        let priority = input
            .get("priority")
            .and_then(|v| v.as_str())
            .and_then(|p| TaskPriority::from_str(p))
            .unwrap_or_default();

        // Create task with priority
        let task = self
            .store
            .create_with_priority(subject, description, active_form, priority)
            .await;

        // Set additional fields if provided
        if let Some(session_id) = input.get("sessionId").and_then(|v| v.as_str()) {
            self.store
                .update(&task.id, |t| t.session_id = Some(session_id.to_string()))
                .await;
        }
        if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
            self.store
                .update(&task.id, |t| t.owner = Some(owner.to_string()))
                .await;
        }
        if let Some(tags) = input.get("tags").and_then(|v| v.as_array()) {
            let tags: Vec<String> = tags
                .iter()
                .filter_map(|t| t.as_str())
                .map(|s| s.to_string())
                .collect();
            self.store.update(&task.id, |t| t.tags = tags).await;
        }

        // Get updated task for response
        let task = match self.store.get(&task.id).await {
            Some(t) => t,
            None => return ToolResult::error("Failed to retrieve created task"),
        };

        let priority_str = task.priority.as_str();
        let display_id = self.store.format_display_id(&task.id).await;
        let progress_str = if task.progress > 0 {
            format!(
                " ({}% - {})",
                task.progress,
                task.progress_message.as_deref().unwrap_or("")
            )
        } else {
            String::new()
        };

        ToolResult::success(format!(
            "Task #{} created successfully: {} [{}]{progress_str}\nDescription: {}",
            display_id, task.subject, priority_str, task.description
        ))
    }
}

use aemeath_core::task::{TaskPriority, TaskStore};
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct TaskCreateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str { "TaskCreate" }
    fn description(&self) -> &str {
        "Create a task to track progress on multi-step work.\n\nUsage:\n- subject: A brief, actionable title\n- description: What needs to be done\n- activeForm: Present continuous form shown when in_progress (e.g., \"Running tests\")\n- priority: Task priority (low, normal, high, urgent)\n- sessionId: Session ID to associate with this task"
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
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let subject = match input.get("subject").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error("missing required parameter: subject"),
        };
        let description = match input.get("description").and_then(|v| v.as_str()) {
            Some(d) => d.to_string(),
            None => return ToolResult::error("missing required parameter: description"),
        };
        let active_form = input.get("activeForm").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Parse priority
        let priority = input.get("priority")
            .and_then(|v| v.as_str())
            .and_then(|p| TaskPriority::from_str(p))
            .unwrap_or_default();

        // Create task with priority
        let task = self.store.create_with_priority(subject, description, active_form, priority).await;

        // Set additional fields if provided
        if let Some(session_id) = input.get("sessionId").and_then(|v| v.as_str()) {
            self.store.update(&task.id, |t| t.session_id = Some(session_id.to_string())).await;
        }
        if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
            self.store.update(&task.id, |t| t.owner = Some(owner.to_string())).await;
        }
        if let Some(tags) = input.get("tags").and_then(|v| v.as_array()) {
            let tags: Vec<String> = tags.iter()
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
        let progress_str = if task.progress > 0 {
            format!(" ({}% - {})", task.progress, task.progress_message.as_deref().unwrap_or(""))
        } else {
            String::new()
        };

        ToolResult::success(format!(
            "Task #{} created successfully: {} [{}]{progress_str}\nDescription: {}",
            task.id, task.subject, priority_str, task.description
        ))
    }
}

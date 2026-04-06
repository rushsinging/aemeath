use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use aemeath_core::task::{TaskStatus, TaskStore};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// TodoWrite tool - manages a todo list for tracking progress on multi-step tasks.
/// This is similar to TaskCreate/Update but simpler, focused on quick todo tracking.
pub struct TodoWriteTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str { "TodoWrite" }
    fn description(&self) -> &str {
        "Create a todo list to track progress on multi-step work. Use this to plan and track tasks during complex operations.\n\nUsage:\n- Create todos at the start of complex tasks\n- Update status as you progress (pending → in_progress → completed)\n- Use 'activeForm' to show current action (e.g., \"Running tests\")\n- Delete completed todos when done"
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Todo ID (auto-generated if not provided)"
                            },
                            "subject": {
                                "type": "string",
                                "description": "Brief, actionable title"
                            },
                            "description": {
                                "type": "string",
                                "description": "What needs to be done"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"],
                                "description": "Task status"
                            },
                            "activeForm": {
                                "type": "string",
                                "description": "Present continuous form shown when in_progress (e.g., \"Running tests\")"
                            }
                        },
                        "required": ["subject"]
                    },
                    "description": "List of todos to create or update"
                }
            },
            "required": ["todos"]
        })
    }
    fn is_read_only(&self) -> bool { false }
    fn is_concurrency_safe(&self) -> bool { true }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let todos = match input["todos"].as_array() {
            Some(arr) => arr,
            None => return ToolResult::error("Todos array is required"),
        };

        if todos.is_empty() {
            // Empty array means clear all todos
            let all_tasks = self.store.list().await;
            for task in all_tasks {
                self.store.update(&task.id, |t| {
                    t.status = TaskStatus::Deleted;
                }).await;
            }
            return ToolResult::success("All todos cleared");
        }

        let mut results: Vec<String> = Vec::new();

        for todo in todos {
            let id = todo["id"].as_str();
            let subject = todo["subject"].as_str().unwrap_or("");
            let description = todo["description"].as_str().unwrap_or("");
            let status_str = todo["status"].as_str().unwrap_or("pending");
            let active_form = todo["activeForm"].as_str();

            if subject.is_empty() {
                results.push("Skipped todo with empty subject".to_string());
                continue;
            }

            let status = match status_str {
                "pending" => TaskStatus::Pending,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                _ => TaskStatus::Pending,
            };

            if let Some(id_str) = id {
                // Update existing todo
                if self.store.update(id_str, |t| {
                    t.subject = subject.to_string();
                    t.description = description.to_string();
                    t.status = status.clone();
                    if let Some(af) = active_form {
                        t.active_form = Some(af.to_string());
                    }
                }).await.is_some() {
                    results.push(format!("Updated todo #{}: {}", id_str, subject));
                } else {
                    // Create new with specified ID
                    let task = self.store.create(
                        subject.to_string(),
                        description.to_string(),
                        active_form.map(|s| s.to_string()),
                    ).await;
                    // Update status
                    self.store.update(&task.id, |t| {
                        t.status = status;
                    }).await;
                    results.push(format!("Created todo #{}: {}", task.id, subject));
                }
            } else {
                // Create new todo
                let task = self.store.create(
                    subject.to_string(),
                    description.to_string(),
                    active_form.map(|s| s.to_string()),
                ).await;

                // Update status if not pending
                if status != TaskStatus::Pending {
                    self.store.update(&task.id, |t| {
                        t.status = status;
                    }).await;
                }

                results.push(format!("Created todo #{}: {}", task.id, subject));
            }
        }

        // Show current todo list
        let all_tasks = self.store.list().await;
        let pending = all_tasks.iter().filter(|t| t.status == TaskStatus::Pending).count();
        let in_progress = all_tasks.iter().filter(|t| t.status == TaskStatus::InProgress).count();
        let completed = all_tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();

        let summary = format!(
            "\n\nTodo list summary: {} pending, {} in_progress, {} completed",
            pending, in_progress, completed
        );

        ToolResult::success(results.join("\n") + &summary)
    }
}

use async_trait::async_trait;
use serde_json::Value;
use share::tool::{Tool, ToolContext, ToolResult};
use std::sync::Arc;
use storage::api::{TaskPriority, TaskStatus, TaskStore};

pub struct TaskListTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "TaskList"
    }
    fn description(&self) -> &str {
        "List all tasks and their status. Use to discover available work.\n\n\
         Look for tasks that are pending with no unresolved blocked_by dependencies — \
         these are ready to execute. You can work on them directly or launch Agent \
         for tasks that can run in parallel.\n\n\
         Call this after completing a task to find the next one to work on."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "Filter by status"
                },
                "priority": {
                    "type": "string",
                    "enum": ["low", "normal", "high", "urgent"],
                    "description": "Filter by priority"
                },
                "sessionId": {
                    "type": "string",
                    "description": "Filter by session ID"
                }
            },
        })
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        // Parse filters
        let status_filter = input
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "pending" => Some(TaskStatus::Pending),
                "in_progress" => Some(TaskStatus::InProgress),
                "completed" => Some(TaskStatus::Completed),
                "deleted" => Some(TaskStatus::Deleted),
                _ => None,
            });

        let priority_filter = input
            .get("priority")
            .and_then(|v| v.as_str())
            .and_then(TaskPriority::parse);

        let session_filter = input.get("sessionId").and_then(|v| v.as_str());

        // Get tasks with filters
        let mut tasks = self.store.list().await;

        if let Some(status) = status_filter {
            tasks.retain(|t| t.status == status);
        }
        if let Some(priority) = priority_filter {
            tasks.retain(|t| t.priority == priority);
        }
        if let Some(session_id) = session_filter {
            tasks.retain(|t| t.session_id.as_deref() == Some(session_id));
        }

        if tasks.is_empty() {
            return ToolResult::success("No tasks found");
        }

        let stats = self.store.stats().await;
        let mut output = format!(
            "Tasks: {} total ({} pending, {} in_progress, {} completed)\n\n",
            stats.total - stats.deleted,
            stats.pending,
            stats.in_progress,
            stats.completed
        );

        let batches = self.store.list_batches().await;
        for batch in batches {
            let batch_tasks = self
                .store
                .tasks_in_batch(
                    batch.id,
                    &[
                        TaskStatus::Pending,
                        TaskStatus::InProgress,
                        TaskStatus::Completed,
                    ],
                )
                .await;
            if batch_tasks.is_empty() {
                continue;
            }
            let done = batch_tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Completed)
                .count();
            output.push_str(&format!(
                "Task list #{} [{:?}] — {}/{} done{}\n",
                batch.id,
                batch.status,
                done,
                batch_tasks.len(),
                batch
                    .summary
                    .as_deref()
                    .map(|summary| format!(" — {summary}"))
                    .unwrap_or_default()
            ));
        }

        for task in &tasks {
            let display_id = self.store.format_display_id(&task.id).await;
            let icon = match task.status {
                TaskStatus::Pending => "□",
                TaskStatus::InProgress => "■",
                TaskStatus::Completed => "✓",
                TaskStatus::Deleted => "✗",
            };
            let status_label = match task.status {
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Completed => "completed",
                TaskStatus::Deleted => "deleted",
            };
            let priority_label = match task.priority {
                TaskPriority::Urgent => " [urgent]",
                TaskPriority::High => " [high]",
                TaskPriority::Normal => "",
                TaskPriority::Low => " [low]",
            };
            let progress = if task.progress > 0 {
                format!(" [{}%]", task.progress)
            } else {
                "".to_string()
            };
            let blocked = if self.store.is_blocked(task).await {
                " blocked"
            } else if !task.blocked_by.is_empty() {
                " waiting"
            } else {
                ""
            };
            let owner = task
                .owner
                .as_deref()
                .map(|o| format!(" (@{})", o))
                .unwrap_or_default();

            output.push_str(&format!(
                "{} #{} {}{}{} [{}]{}{}{}\n   {}\n",
                icon,
                display_id,
                task.subject,
                priority_label,
                progress,
                status_label,
                owner,
                blocked,
                if !task.tags.is_empty() {
                    format!(" [{}]", task.tags.join(", "))
                } else {
                    "".to_string()
                },
                task.description
            ));
        }

        ToolResult::success(output.trim_end())
    }
}

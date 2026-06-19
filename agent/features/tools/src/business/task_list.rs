use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_list::TaskListResult;
use std::sync::Arc;
use storage::api::{TaskPriority, TaskStatus, TaskStore};

pub struct TaskListTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl TypedTool for TaskListTool {
    type Output = TaskListResult;
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

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskListResult> {
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
            return TypedToolResult::success_value(serde_json::json!({
                "status": "success",
                "message": "No tasks found",
                "data": { "tasks": [] }
            }));
        }

        let stats = self.store.stats().await;

        let mut batches_json = serde_json::json!([]);
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
            batches_json
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({
                    "id": batch.id,
                    "status": format!("{:?}", batch.status),
                    "done": done,
                    "total": batch_tasks.len(),
                    "summary": batch.summary
                }));
        }

        let mut tasks_json = serde_json::json!([]);
        for task in &tasks {
            let display_id = self.store.format_display_id(&task.id).await;
            let status_label = match task.status {
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Completed => "completed",
                TaskStatus::Deleted => "deleted",
            };
            let priority_label = task.priority.as_str();
            let is_blocked = self.store.is_blocked(task).await;

            let mut task_obj = serde_json::json!({
                "id": display_id,
                "subject": task.subject,
                "status": status_label,
                "priority": priority_label,
                "progress": task.progress,
                "tags": task.tags,
                "description": task.description
            });

            if let Some(ref owner) = task.owner {
                task_obj["owner"] = serde_json::Value::String(owner.clone());
            }

            if !task.blocked_by.is_empty() {
                let dep_displays = self.store.to_display_ids(&task.blocked_by).await;
                task_obj["blocked_by"] = serde_json::json!(dep_displays);
                task_obj["is_blocked"] = serde_json::json!(is_blocked);
            }

            if !task.blocks.is_empty() {
                let dep_displays = self.store.to_display_ids(&task.blocks).await;
                task_obj["blocks"] = serde_json::json!(dep_displays);
            }

            tasks_json.as_array_mut().unwrap().push(task_obj);
        }

        let msg = format!(
            "{} tasks ({} pending, {} in_progress, {} completed)",
            stats.total - stats.deleted,
            stats.pending,
            stats.in_progress,
            stats.completed
        );

        TypedToolResult::success_value(serde_json::json!({
            "status": "success",
            "message": msg,
            "data": {
                "batches": batches_json,
                "tasks": tasks_json
            }
        }))
    }
}

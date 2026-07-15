use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_update::{TaskUpdateInput, TaskUpdateResult};
use std::sync::Arc;
use storage::api::{TaskPriority, TaskStatus, TaskStore};

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
        "Update a single field on a task.\n\n\
         Pass `key` to select which field to change and `value` for the new value. \
         Each call updates exactly one field. Value is always a string.\n\n\
         Valid keys: status, subject, description, owner, priority, blocked_by_id.\n\n\
         Status workflow: pending → in_progress → completed. Use 'deleted' to remove.\n\n\
         When you mark a task as completed, the system will show which downstream tasks \
         are now unblocked and ready to execute. Use this to decide what to work on next.\n\n\
         After completing a task, check the unblocked list or call TaskList to find the next available task."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_update(lang))
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskUpdateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskUpdateResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        // Mutates persistent task state; keep ordered with related task operations.
        false
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskUpdateResult> {
        let args: TaskUpdateInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let input_id = args.task_id.clone();

        // Resolve display number (batch-local id) to global task id
        let task_id = match self.store.resolve_display_id(&input_id).await {
            Some(global_id) => global_id,
            None => return TypedToolResult::error(format!("task not found: {input_id}")),
        };

        let key = args.key.as_str();
        let val = &args.value;

        // Validate: value must be a string for all keys
        let val_str = match val.as_str() {
            Some(s) => s,
            None => {
                return TypedToolResult::error(format!("value must be a string for key '{key}'"))
            }
        };

        // Pre-resolve blocked_by_id display number to global id
        let resolved_dep: Option<String> = match key {
            "blocked_by_id" => {
                let gid = self.store.resolve_display_id(val_str).await;
                if gid.is_none() {
                    return TypedToolResult::error(format!(
                        "blocked_by_id task not found: {val_str}"
                    ));
                }
                gid
            }
            _ => None,
        };

        // Validate key
        match key {
            "status" | "subject" | "description" | "owner" | "priority" | "blocked_by_id" => {}
            _ => {
                return TypedToolResult::error(format!(
                    "unknown field '{key}'. Valid keys: status, subject, description, owner, priority, blocked_by_id"
                ));
            }
        }

        let result = self
            .store
            .update(&task_id, |task| match key {
                "status" => {
                    task.status = match val_str {
                        "pending" => TaskStatus::Pending,
                        "in_progress" => TaskStatus::InProgress,
                        "completed" => TaskStatus::Completed,
                        "deleted" => TaskStatus::Deleted,
                        _ => task.status.clone(),
                    };
                }
                "subject" => {
                    task.subject = val_str.to_string();
                }
                "description" => {
                    task.description = val_str.to_string();
                }
                "owner" => {
                    task.owner = Some(val_str.to_string());
                }
                "priority" => {
                    if let Some(p) = TaskPriority::parse(val_str) {
                        task.priority = p;
                    }
                }
                "blocked_by_id" => {
                    if let Some(gid) = &resolved_dep {
                        if !task.blocked_by.contains(gid) {
                            task.blocked_by.push(gid.clone());
                        }
                    }
                }
                // Unreachable — validated above
                _ => {}
            })
            .await;

        match result {
            Some(task) => {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = format!("{:?}", task.status);
                let priority = task.priority.as_str().to_string();
                let blocked_by = self
                    .store
                    .to_display_ids(&task.blocked_by)
                    .await
                    .into_iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>();

                let message = format!("Task #{} updated. Status: {}", display_id, status);
                TypedToolResult::success(
                    message,
                    TaskUpdateResult {
                        task_id: display_id,
                        status,
                        subject: task.subject.clone(),
                        priority,
                        blocked_by,
                    },
                )
            }
            None => TypedToolResult::error(format!("task not found: {input_id}")),
        }
    }
}

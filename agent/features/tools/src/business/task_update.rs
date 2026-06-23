use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_update::{TaskUpdateInput, TaskUpdateResult};
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
        true
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskUpdateResult> {
        let now = current_timestamp_millis();
        let args: TaskUpdateInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let input_id = args.taskId.clone();

        // Resolve display number (batch-local id) to global task id
        let task_id = match self.store.resolve_display_id(&input_id).await {
            Some(global_id) => global_id,
            None => return TypedToolResult::error(format!("task not found: {input_id}")),
        };

        // Pre-resolve dependency display numbers to global ids (must be async)
        let resolved_blocked_by = if let Some(display_ids) = args.addBlockedBy.as_deref() {
            self.store.resolve_display_ids(display_ids).await
        } else {
            Vec::new()
        };
        let resolved_blocks = if let Some(display_ids) = args.addBlocks.as_deref() {
            self.store.resolve_display_ids(display_ids).await
        } else {
            Vec::new()
        };

        let result = self
            .store
            .update(&task_id, |task| {
                // Status update
                if let Some(status) = args.status.as_deref() {
                    task.status = match status {
                        "pending" => TaskStatus::Pending,
                        "in_progress" => TaskStatus::InProgress,
                        "completed" => TaskStatus::Completed,
                        "deleted" => TaskStatus::Deleted,
                        _ => task.status.clone(),
                    };
                }

                // Basic field updates
                if let Some(subject) = args.subject {
                    task.subject = subject;
                }
                if let Some(desc) = args.description {
                    task.description = desc;
                }
                if let Some(af) = args.activeForm {
                    task.active_form = Some(af);
                }
                if let Some(owner) = args.owner {
                    task.owner = Some(owner);
                }

                // Priority update
                if let Some(priority) = args.priority.as_deref() {
                    if let Some(p) = TaskPriority::parse(priority) {
                        task.priority = p;
                    }
                }

                // Progress update
                if let Some(progress) = args.progress {
                    task.progress = progress.min(100);
                }
                if let Some(msg) = args.progressMessage {
                    task.progress_message = Some(msg);
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
                if let Some(add_tags) = args.addTags {
                    for tag in add_tags {
                        task.add_tag(tag, now);
                    }
                }
                if let Some(remove_tags) = args.removeTags {
                    for tag in remove_tags {
                        task.remove_tag(&tag, now);
                    }
                }
            })
            .await;

        match result {
            Some(task) => {
                let display_id = self.store.format_display_id(&task.id).await;
                let status = format!("{:?}", task.status);

                let message = format!("Task #{} updated", display_id);
                TypedToolResult::success(
                    message,
                    TaskUpdateResult {
                        task_id: display_id,
                        status,
                        subject: task.subject.clone(),
                    },
                )
            }
            None => TypedToolResult::error(format!("task not found: {input_id}")),
        }
    }
}

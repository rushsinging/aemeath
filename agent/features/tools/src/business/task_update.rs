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
        "Update a single field on a task.\n\n\
         Pass `key` to select which field to change and `value` for the new value. \
         Each call updates exactly one field.\n\n\
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

        // ── Validate key & extract typed value ──────────────────────────
        let now = current_timestamp_millis();
        let key = args.key.as_str();
        let val = &args.value;

        // Pre-resolve dependency display numbers to global ids (must be async)
        let resolved_deps: Vec<String> = match key {
            "add_blocked_by" | "add_blocks" => {
                let ids: Vec<String> = match val.as_array() {
                    Some(arr) => arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect(),
                    None => {
                        return TypedToolResult::error(format!(
                            "value for '{key}' must be an array of strings"
                        ))
                    }
                };
                self.store.resolve_display_ids(&ids).await
            }
            _ => Vec::new(),
        };

        // Validate value type per key before entering the closure
        match key {
            "status" | "subject" | "description" | "active_form" | "owner" | "priority"
            | "progress_message" => {
                if !val.is_string() {
                    return TypedToolResult::error(format!("value for '{key}' must be a string"));
                }
            }
            "progress" => {
                if val.as_u64().is_none() {
                    return TypedToolResult::error(format!(
                        "value for '{key}' must be an integer (0-100)"
                    ));
                }
            }
            "add_blocked_by" | "add_blocks" | "add_tags" | "remove_tags" => {
                if !val.is_array() {
                    return TypedToolResult::error(format!(
                        "value for '{key}' must be an array of strings"
                    ));
                }
            }
            _ => {
                return TypedToolResult::error(format!(
                    "unknown field '{key}'. Valid keys: status, subject, description, active_form, owner, priority, progress, progress_message, add_blocked_by, add_blocks, add_tags, remove_tags"
                ));
            }
        }

        let result = self
            .store
            .update(&task_id, |task| match key {
                // ── String fields ───────────────────────────────────────
                "status" => {
                    let s = val.as_str().unwrap_or("");
                    task.status = match s {
                        "pending" => TaskStatus::Pending,
                        "in_progress" => TaskStatus::InProgress,
                        "completed" => TaskStatus::Completed,
                        "deleted" => TaskStatus::Deleted,
                        _ => task.status.clone(),
                    };
                }
                "subject" => {
                    task.subject = val.as_str().unwrap_or("").to_string();
                }
                "description" => {
                    task.description = val.as_str().unwrap_or("").to_string();
                }
                "active_form" => {
                    task.active_form = Some(val.as_str().unwrap_or("").to_string());
                }
                "owner" => {
                    task.owner = Some(val.as_str().unwrap_or("").to_string());
                }
                "priority" => {
                    if let Some(p) = TaskPriority::parse(val.as_str().unwrap_or("")) {
                        task.priority = p;
                    }
                }
                "progress_message" => {
                    task.progress_message = Some(val.as_str().unwrap_or("").to_string());
                }
                // ── Integer fields ──────────────────────────────────────
                "progress" => {
                    task.progress = (val.as_u64().unwrap_or(0) as u8).min(100);
                }
                // ── Dependency fields (pre-resolved) ────────────────────
                "add_blocked_by" => {
                    for gid in &resolved_deps {
                        if !task.blocked_by.contains(gid) {
                            task.blocked_by.push(gid.clone());
                        }
                    }
                }
                "add_blocks" => {
                    for gid in &resolved_deps {
                        if !task.blocks.contains(gid) {
                            task.blocks.push(gid.clone());
                        }
                    }
                }
                // ── Tag fields ──────────────────────────────────────────
                "add_tags" => {
                    if let Some(arr) = val.as_array() {
                        for tag in arr.iter().filter_map(|v| v.as_str()) {
                            task.add_tag(tag.to_string(), now);
                        }
                    }
                }
                "remove_tags" => {
                    if let Some(arr) = val.as_array() {
                        for tag in arr.iter().filter_map(|v| v.as_str()) {
                            task.remove_tag(tag, now);
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

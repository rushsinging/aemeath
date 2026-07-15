use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_create::{TaskCreateInput, TaskCreateResult};
use std::sync::Arc;
use storage::api::{TaskPriority, TaskStore};

/// 判断是否为占位符值：纯空白（如 `""` `"  "`）。
fn is_placeholder(val: &str) -> bool {
    val.trim().is_empty()
}

pub struct TaskCreateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl TypedTool for TaskCreateTool {
    type Output = TaskCreateResult;
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn description(&self) -> &str {
        "Create a task to track progress on complex multi-step work only.\n\n\
           Use task management only when the user request requires at least 3 substantial execution steps,\n\
           multiple dependent changes, or parallel sub-agent coordination. Do NOT create tasks for simple\n\
           one-step requests such as answering a question, inspecting a file, checking bug status, running a\n\
           single command, or making a tiny localized edit. For simple requests, execute directly.\n\n\
           IMPORTANT: each task must be a SINGLE, CONCRETE, VERIFIABLE step. BAD tasks lump multiple\n\
           changes together, such as \"Implement and verify feature\" or \"Fix all related issues\". GOOD tasks\n\
           are specific: \"Read X.rs to understand current error handling\", \"Add retry logic to Y::send\",\n\
           \"Add unit test for Z edge case\", \"Run cargo clippy and fix warnings\". When a task involves\n\
           implementation, split it into per-file or per-function changes plus separate verification steps.\n\n\
           IMPORTANT workflow when task management is actually needed:\n\
           1. First, describe your complete plan as text — list ALL planned tasks so the user can see the full picture\n\
           2. For a new complex multi-step user request, call TaskListCreate before TaskCreate so tasks attach to a request summary\n\
           3. Then create tasks one by one with TaskCreate\n\
           4. Use TaskUpdate to set dependencies and assign agents\n\n\
           After creating tasks, use TaskUpdate to:\n\
           - Set dependencies (blocked_by_id) between tasks\n\
           - Mark as in_progress before starting work\n\
           - Mark as completed when done — the system will show which tasks are unblocked\n\n\
           Use TaskList to discover pending tasks with no unresolved dependencies.\n\
            Launch Agent for independent tasks that can run in parallel."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_create(lang))
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskCreateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskCreateResult::data_schema()
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
    ) -> TypedToolResult<TaskCreateResult> {
        let args: TaskCreateInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": format!("invalid input: {e}"),
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };

        let subject = args.subject;
        let description = args.description;

        // Parse priority
        let priority = args
            .priority
            .as_deref()
            .and_then(TaskPriority::parse)
            .unwrap_or_default();

        // Create task with priority
        let task = self
            .store
            .create_with_priority(subject, description, priority)
            .await;

        // Set additional fields if provided — skip blank/placeholder strings to avoid dirty data (#979)
        if let Some(session_id) = args.session_id {
            if !is_placeholder(&session_id) {
                self.store
                    .update(&task.id, |t| t.session_id = Some(session_id))
                    .await;
            }
        }
        if let Some(owner) = args.owner {
            if !is_placeholder(&owner) {
                self.store.update(&task.id, |t| t.owner = Some(owner)).await;
            }
        }

        // Get updated task for response
        let task = match self.store.get(&task.id).await {
            Some(t) => t,
            None => {
                return TypedToolResult::error(
                    serde_json::json!({
                        "status": "error",
                        "message": "Failed to retrieve created task",
                        "data": {}
                    })
                    .to_string(),
                )
            }
        };

        let display_id = self.store.format_display_id(&task.id).await;

        TypedToolResult::success(
            format!("Task #{} created: {}", display_id, task.subject),
            TaskCreateResult {
                task_id: task.id,
                display_id,
                subject: task.subject.clone(),
                status: format!("{:?}", task.status).to_lowercase(),
                priority: format!("{:?}", task.priority).to_lowercase(),
            },
        )
    }
}

#[cfg(test)]
#[path = "task_create_tests.rs"]
mod tests;

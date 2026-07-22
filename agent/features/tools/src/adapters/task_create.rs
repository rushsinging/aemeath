use crate::domain::types::task_create::{TaskCreateInput, TaskCreateResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{TaskAccess, TaskCreateSpec, TaskPriority};

pub struct TaskCreateTool {
    pub access: Arc<dyn TaskAccess>,
}

fn parse_priority(value: Option<&str>) -> TaskPriority {
    match value.map(str::to_ascii_lowercase).as_deref() {
        Some("low") => TaskPriority::Low,
        Some("high") => TaskPriority::High,
        Some("urgent" | "critical") => TaskPriority::Urgent,
        _ => TaskPriority::Normal,
    }
}

fn status_label(status: task::TaskStatus) -> &'static str {
    match status {
        task::TaskStatus::Pending => "pending",
        task::TaskStatus::InProgress => "in_progress",
        task::TaskStatus::Completed => "completed",
        task::TaskStatus::Deleted => "deleted",
    }
}

fn priority_label(priority: TaskPriority) -> &'static str {
    match priority {
        TaskPriority::Low => "low",
        TaskPriority::Normal => "normal",
        TaskPriority::High => "high",
        TaskPriority::Urgent => "urgent",
    }
}

#[async_trait]
impl TypedTool for TaskCreateTool {
    type Output = TaskCreateResult;
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn description(&self) -> &str {
        "Create a task to track progress on complex multi-step work only.\n\n\
         Call TaskListCreate before TaskCreate so the task is attached to the active request batch.\n\
         Each task must be a single, concrete, verifiable step."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_create(lang))
    }
    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskCreateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskCreateResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskCreateResult> {
        let args: TaskCreateInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let priority = parse_priority(args.priority.as_deref());
        let spec = match TaskCreateSpec::try_new(args.subject, args.description, None, priority) {
            Ok(spec) => spec,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        // Task BC deliberately returns NoActiveBatch; the tool must not create one implicitly.
        let created = match self
            .access
            .create_task(spec, chrono::Utc::now().timestamp_millis() as u64)
        {
            Ok(result) => result.value,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let display_id = created.seq().to_string();
        TypedToolResult::success(
            format!("Task #{} created: {}", display_id, created.subject()),
            TaskCreateResult {
                task_id: display_id.clone(),
                display_id,
                subject: created.subject().to_owned(),
                status: status_label(created.status()).to_owned(),
                priority: priority_label(created.priority()).to_owned(),
            },
        )
    }
}

#[cfg(test)]
#[path = "task_create_tests.rs"]
mod tests;

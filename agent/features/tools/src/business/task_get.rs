use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_get::{TaskGetInput, TaskGetResult};
use std::sync::Arc;
use task::{TaskAccess, TaskId};

pub struct TaskGetTool {
    pub access: Arc<dyn TaskAccess>,
}

fn parse_task_id(value: &str) -> Result<TaskId, String> {
    value
        .parse::<u64>()
        .map(TaskId::new)
        .map_err(|_| format!("Task ID must be a decimal number: {value}"))
}

/// Temporary anti-corruption mapping while tool results retain the legacy wire DTO.
fn to_legacy_task(task: &task::Task) -> share::tool::types::task::Task {
    use share::tool::types::task::{TaskPriority, TaskStatus};
    share::tool::types::task::Task {
        id: task.id().to_string(),
        subject: task.subject().to_owned(),
        description: task.description().to_owned(),
        status: match task.status() {
            task::TaskStatus::Pending => TaskStatus::Pending,
            task::TaskStatus::InProgress => TaskStatus::InProgress,
            task::TaskStatus::Completed => TaskStatus::Completed,
            task::TaskStatus::Deleted => TaskStatus::Deleted,
        },
        owner: None,
        blocked_by: task.blocked_by().iter().map(ToString::to_string).collect(),
        priority: match task.priority() {
            task::TaskPriority::Low => TaskPriority::Low,
            task::TaskPriority::Normal => TaskPriority::Normal,
            task::TaskPriority::High => TaskPriority::High,
            task::TaskPriority::Urgent => TaskPriority::Urgent,
        },
        created_at: task.created_at(),
        updated_at: task.updated_at(),
        session_id: task.session_id().map(str::to_owned),
        batch: task.batch().get(),
    }
}

#[async_trait]
impl TypedTool for TaskGetTool {
    type Output = TaskGetResult;
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn description(&self) -> &str {
        "Retrieve a task by ID. Returns task details including subject, description, status, and dependencies."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_get(lang))
    }
    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskGetInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskGetResult::data_schema()
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskGetResult> {
        let args: TaskGetInput = match serde_json::from_value(input) {
            Ok(args) => args,
            Err(error) => return TypedToolResult::error(format!("invalid input: {error}")),
        };
        let id = match parse_task_id(&args.task_id) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };
        let task = match self.access.get(id) {
            Some(task) => task,
            None => return TypedToolResult::error(format!("Task not found: {}", args.task_id)),
        };
        TypedToolResult::success(
            format!("Task #{}: {}", task.id(), task.subject()),
            TaskGetResult {
                task: to_legacy_task(&task),
            },
        )
    }
}

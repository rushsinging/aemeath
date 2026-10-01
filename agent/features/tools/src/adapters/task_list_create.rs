use crate::domain::types::task_list_create::{TaskListCreateInput, TaskListCreateResult};
use crate::domain::{CommittedTaskChange, ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::{BatchCreateSpecData, TaskAccess};

pub struct TaskListCreateTool {
    pub access: Arc<dyn TaskAccess>,
}

#[async_trait]
impl TypedTool for TaskListCreateTool {
    type Output = TaskListCreateResult;
    fn name(&self) -> &str {
        "TaskListCreate"
    }

    fn description(&self) -> &str {
        share::i18n::tools::task::task_list_create("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_list_create(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListCreateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListCreateResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        // Mutates the active task list; must remain ordered with task writes.
        false
    }

    fn timeout_secs(&self) -> u64 {
        5
    }

    async fn call(
        &self,
        input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskListCreateResult> {
        let args: TaskListCreateInput = match serde_json::from_value(input) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let subject = args.subject;
        let spec = match BatchCreateSpecData::try_new(args.summary) {
            Ok(spec) => spec,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let command_result = match self
            .access
            .create_batch(spec, chrono::Utc::now().timestamp_millis() as u64)
        {
            Ok(result) => result,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let task_change = CommittedTaskChange::from_command_result(&command_result);
        let batch = command_result.value;
        let batch_id = batch.id().to_string();
        TypedToolResult::success(
            format!("Task list #{} created. Subject: {}", batch_id, subject),
            TaskListCreateResult { batch_id },
        )
        .with_task_change(task_change)
    }
}

#[cfg(test)]
#[path = "task_list_create_tests.rs"]
mod tests;

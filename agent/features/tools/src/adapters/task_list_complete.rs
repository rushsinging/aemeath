use crate::domain::types::task_list_complete::TaskListCompleteResult;
use crate::domain::{CommittedTaskChange, ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use task::TaskAccess;

pub struct TaskListCompleteTool {
    pub access: Arc<dyn TaskAccess>,
}

#[async_trait]
impl TypedTool for TaskListCompleteTool {
    type Output = TaskListCompleteResult;
    fn name(&self) -> &str {
        "TaskListComplete"
    }

    fn description(&self) -> &str {
        share::i18n::tools::task::task_list_complete("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_list_complete(lang))
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        TaskListCompleteResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        // Mutates the active task list; must remain ordered with task writes.
        false
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<TaskListCompleteResult> {
        let Some(batch_id) = self.access.lifecycle_snapshot(0).current_batch else {
            return TypedToolResult::error("no active task list");
        };
        match self.access.archive_batch(batch_id) {
            Ok(command_result) => {
                let task_change = CommittedTaskChange::from_command_result(&command_result);
                let batch_id = command_result.value.id().to_string();
                TypedToolResult::success(
                    format!("Task list #{} completed", batch_id),
                    TaskListCompleteResult { batch_id },
                )
                .with_task_change(task_change)
            }
            Err(error) => TypedToolResult::error(error.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "task_list_complete_tests.rs"]
mod tests;

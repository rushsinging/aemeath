use crate::domain::types::task_list_complete::TaskListCompleteResult;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
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
        "Complete the current active task list after all tasks for the current user request are done. This stops future reminders for that completed list."
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
            Ok(result) => {
                let batch_id = result.value.id().to_string();
                TypedToolResult::success(
                    format!("Task list #{} completed", batch_id),
                    TaskListCompleteResult { batch_id },
                )
            }
            Err(error) => TypedToolResult::error(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolExecutionContext {
        crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from(
            ".",
        ))
        .build()
    }

    fn create_batch(access: &dyn task::TaskAccess) -> task::Batch {
        access
            .create_batch(
                task::BatchCreateSpec::try_new("当前请求".to_string()).unwrap(),
                1,
            )
            .unwrap()
            .value
    }

    #[tokio::test]
    async fn test_task_list_complete_success_archives_current_batch() {
        let access = Arc::new(task::TaskStore::new());
        let access: Arc<dyn task::TaskAccess> = access.clone();
        let batch = create_batch(access.as_ref());
        let tool = TaskListCompleteTool {
            access: access.clone(),
        };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.data.unwrap().batch_id, batch.id().to_string());
        assert_eq!(
            task::TaskAccess::list_batches(access.as_ref())[0].status(),
            task::BatchStatus::Archived
        );
    }

    #[tokio::test]
    async fn test_task_list_complete_without_active_list_errors() {
        let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
        let tool = TaskListCompleteTool { access };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(result.is_error);
        assert!(result.text.contains("no active task list"));
    }

    #[tokio::test]
    async fn test_task_list_complete_keeps_task_batch() {
        let access = Arc::new(task::TaskStore::new());
        let access: Arc<dyn task::TaskAccess> = access.clone();
        let batch = create_batch(access.as_ref());
        let created = access
            .create_task(
                task::TaskCreateSpec::try_new(
                    "任务".to_string(),
                    "描述".to_string(),
                    None,
                    task::TaskPriority::Normal,
                )
                .unwrap(),
                2,
            )
            .unwrap()
            .value;
        let tool = TaskListCompleteTool {
            access: access.clone(),
        };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(!result.is_error, "{}", result.text);
        assert_eq!(access.get(created.id()).unwrap().batch(), batch.id());
    }
}

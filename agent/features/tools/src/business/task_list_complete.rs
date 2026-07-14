use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_list_complete::TaskListCompleteResult;
use std::sync::Arc;
use storage::api::TaskStore;

pub struct TaskListCompleteTool {
    pub store: Arc<TaskStore>,
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
        use share::tool::types::ToolSchema;
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
        match self.store.complete_list().await {
            Some(batch) => TypedToolResult::success(
                format!("Task list #{} completed", batch.id),
                TaskListCompleteResult {
                    batch_id: batch.id.to_string(),
                },
            ),
            None => TypedToolResult::error("no active task list"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::api::BatchStatus;

    fn test_ctx() -> ToolExecutionContext {
        ToolExecutionContext {
            workspace: project::api::WorkspaceService::new(std::path::PathBuf::from(".")),
            cancel: tokio_util::sync::CancellationToken::new(),
            read_files: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashSet::new()),
            ),
            resources: crate::api::ToolResources {
                agent_runner: None,
                registry: None,
                memory_config: share::config::MemoryConfig::default(),
                lang: "en".to_string(),
                allow_all: false,
            },
            session_reminders: None,
            plan_mode: None,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        }
    }

    #[tokio::test]
    async fn test_task_list_complete_success_archives_current_batch() {
        let store = Arc::new(TaskStore::new());
        store
            .create_list("当前".to_string(), "当前请求".to_string())
            .await;
        let tool = TaskListCompleteTool {
            store: store.clone(),
        };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(!result.is_error);
        assert_eq!(store.list_batches().await[0].status, BatchStatus::Archived);
    }

    #[tokio::test]
    async fn test_task_list_complete_without_active_list_errors() {
        let store = Arc::new(TaskStore::new());
        let tool = TaskListCompleteTool { store };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(result.is_error);
        assert!(result.text.contains("no active task list"));
    }

    #[tokio::test]
    async fn test_task_list_complete_keeps_task_batch() {
        let store = Arc::new(TaskStore::new());
        let list = store
            .create_list("当前".to_string(), "当前请求".to_string())
            .await;
        let task = store.create("任务".to_string(), "描述".to_string()).await;
        let tool = TaskListCompleteTool {
            store: store.clone(),
        };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(!result.is_error);
        assert_eq!(store.get(&task.id).await.unwrap().batch, list.id);
    }
}

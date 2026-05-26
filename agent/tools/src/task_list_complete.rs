use async_trait::async_trait;
use serde_json::Value;
use share::task_ops::TaskStore;
use share::tool::{Tool, ToolContext, ToolResult};
use std::sync::Arc;

pub struct TaskListCompleteTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskListCompleteTool {
    fn name(&self) -> &str {
        "TaskListComplete"
    }

    fn description(&self) -> &str {
        "Complete the current active task list after all tasks for the current user request are done. This stops future reminders for that completed list."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
        match self.store.complete_list().await {
            Some(batch) => ToolResult::success(format!("Task list #{} completed", batch.id)),
            None => ToolResult::error("no active task list"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::task_ops::BatchStatus;

    fn test_ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
            working_root: std::sync::Arc::new(std::sync::Mutex::new(std::path::PathBuf::from("."))),
            path_base: std::sync::Arc::new(std::sync::Mutex::new(std::path::PathBuf::from("."))),
            cancel: tokio_util::sync::CancellationToken::new(),
            read_files: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashSet::new()),
            ),
            agent_runner: None,
            session_reminders: None,
            memory_config: share::config::MemoryConfig::default(),
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
            context_stack: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
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
        assert!(result.output.contains("no active task list"));
    }

    #[tokio::test]
    async fn test_task_list_complete_keeps_task_batch() {
        let store = Arc::new(TaskStore::new());
        let list = store
            .create_list("当前".to_string(), "当前请求".to_string())
            .await;
        let task = store
            .create("任务".to_string(), "描述".to_string(), None)
            .await;
        let tool = TaskListCompleteTool {
            store: store.clone(),
        };

        let result = tool.call(serde_json::json!({}), &test_ctx()).await;

        assert!(!result.is_error);
        assert_eq!(store.get(&task.id).await.unwrap().batch, list.id);
    }
}

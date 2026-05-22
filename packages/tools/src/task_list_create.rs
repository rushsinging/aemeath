use aemeath_core::task::TaskStore;
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct TaskListCreateTool {
    pub store: Arc<TaskStore>,
}

#[async_trait]
impl Tool for TaskListCreateTool {
    fn name(&self) -> &str {
        "TaskListCreate"
    }

    fn description(&self) -> &str {
        "Create a task list for one coherent complex user request. Use before TaskCreate only when starting complex multi-step work that has at least 3 substantial execution steps, multiple dependent changes, or parallel sub-agent coordination. Do NOT use for simple one-step requests such as answering a question, inspecting a file, checking bug status, running a single command, or making a tiny localized edit; for those, execute directly without task management. The summary helps future reminders avoid overriding unrelated new user requests."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "subject": { "type": "string", "description": "Short title for this task list" },
                "summary": { "type": "string", "description": "One-sentence summary of the user request this task list belongs to" }
            },
            "required": ["subject", "summary"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let subject = match input.get("subject").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error("missing required parameter: subject"),
        };
        let summary = match input.get("summary").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolResult::error("missing required parameter: summary"),
        };

        let batch = self.store.create_list(subject, summary).await;
        ToolResult::success(format!(
            "Task list #{} created\nSummary: {}",
            batch.id,
            batch.summary.unwrap_or_default()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
            path_base: std::sync::Arc::new(std::sync::Mutex::new(std::path::PathBuf::from("."))),
            cancel: tokio_util::sync::CancellationToken::new(),
            read_files: std::sync::Arc::new(
                std::sync::Mutex::new(std::collections::HashSet::new()),
            ),
            agent_runner: None,
            session_reminders: None,
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 4,
            max_agent_concurrency: 4,
            agent_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: None,
        }
    }

    #[tokio::test]
    async fn test_task_list_create_success_sets_summary() {
        let store = Arc::new(TaskStore::new());
        let tool = TaskListCreateTool {
            store: store.clone(),
        };

        let result = tool
            .call(
                serde_json::json!({"subject": "修复 bug", "summary": "修复 task 状态"}),
                &test_ctx(),
            )
            .await;

        assert!(!result.is_error);
        assert!(result.output.contains("Task list #0 created"));
        assert_eq!(
            store.active_list().await.unwrap().summary.as_deref(),
            Some("修复 task 状态")
        );
    }

    #[tokio::test]
    async fn test_task_list_create_missing_summary_errors() {
        let store = Arc::new(TaskStore::new());
        let tool = TaskListCreateTool { store };

        let result = tool
            .call(serde_json::json!({"subject": "修复 bug"}), &test_ctx())
            .await;

        assert!(result.is_error);
        assert!(result.output.contains("summary"));
    }

    #[tokio::test]
    async fn test_task_list_create_allows_task_create_membership_by_batch() {
        let store = Arc::new(TaskStore::new());
        let tool = TaskListCreateTool {
            store: store.clone(),
        };

        tool.call(
            serde_json::json!({"subject": "当前", "summary": "当前请求"}),
            &test_ctx(),
        )
        .await;
        let task = store
            .create("任务".to_string(), "描述".to_string(), None)
            .await;

        assert_eq!(store.active_list().await.unwrap().id, task.batch);
    }
}

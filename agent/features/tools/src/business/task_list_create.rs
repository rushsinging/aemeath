use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::task_list_create::{TaskListCreateInput, TaskListCreateResult};
use std::sync::Arc;
use task::{BatchCreateSpec, TaskAccess};

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
        "Create a task list for a complex multi-step request (3+ steps, multiple dependencies, or parallel sub-agent coordination). Tasks created afterwards auto-attach to this list."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::task::task_list_create(lang))
    }

    fn input_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        TaskListCreateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
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
        let spec = match BatchCreateSpec::try_new(args.summary) {
            Ok(spec) => spec,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let batch = match self
            .access
            .create_batch(spec, chrono::Utc::now().timestamp_millis() as u64)
        {
            Ok(result) => result.value,
            Err(error) => return TypedToolResult::error(error.to_string()),
        };
        let batch_id = batch.id().to_string();
        TypedToolResult::success(
            format!("Task list #{} created. Subject: {}", batch_id, subject),
            TaskListCreateResult { batch_id },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx() -> ToolExecutionContext {
        ToolExecutionContext {
            workspace: project::wire_production_workspace(std::path::PathBuf::from("."))
                .expect("workspace initialization")
                .into_views(),
            run_id: "test-run".to_string(),
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
    async fn test_task_list_create_success_uses_summary_only() {
        let access = Arc::new(task::TaskStore::new());
        let access: Arc<dyn task::TaskAccess> = access.clone();
        let tool = TaskListCreateTool {
            access: access.clone(),
        };

        let result = tool
            .call(
                serde_json::json!({"subject": "legacy display", "summary": "修复 task 状态"}),
                &test_ctx(),
            )
            .await;

        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.contains("Subject: legacy display"));
        let batches = task::TaskAccess::list_batches(access.as_ref());
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].summary(), Some("修复 task 状态"));
        assert_eq!(result.data.unwrap().batch_id, batches[0].id().to_string());
    }

    #[tokio::test]
    async fn test_task_list_create_missing_summary_errors() {
        let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
        let tool = TaskListCreateTool { access };

        let result = tool
            .call(serde_json::json!({"subject": "修复 bug"}), &test_ctx())
            .await;

        assert!(result.is_error);
        assert!(
            result.text.contains("summary") || result.text.contains("摘要"),
            "{}",
            result.text
        );
    }

    #[tokio::test]
    async fn test_task_list_create_allows_task_create_membership_by_batch() {
        let access = Arc::new(task::TaskStore::new());
        let access: Arc<dyn task::TaskAccess> = access.clone();
        let tool = TaskListCreateTool {
            access: access.clone(),
        };

        let result = tool
            .call(
                serde_json::json!({"subject": "当前", "summary": "当前请求"}),
                &test_ctx(),
            )
            .await;
        assert!(!result.is_error, "{}", result.text);
        let task = access
            .create_task(
                task::TaskCreateSpec::try_new(
                    "任务".to_string(),
                    "描述".to_string(),
                    None,
                    task::TaskPriority::Normal,
                )
                .unwrap(),
                1,
            )
            .unwrap()
            .value;

        assert_eq!(task.batch().to_string(), result.data.unwrap().batch_id);
    }

    #[test]
    fn test_task_list_create_timeout_is_short_for_memory_only_tool() {
        let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
        let tool = TaskListCreateTool { access };

        assert_eq!(tool.timeout_secs(), 5);
    }
}

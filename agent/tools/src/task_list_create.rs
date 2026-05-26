use async_trait::async_trait;
use serde_json::Value;
use share::task_ops::TaskStore;
use share::tool::{Tool, ToolContext, ToolResult};
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

    /// 复现 bug #70：用户反馈 TaskListCreate 首次调用会超时。
    /// 本测试在工具层直接计时，验证「首次调用」与「后续调用」是否存在数量级差异。
    /// 若首次也是毫秒级，证明工具实现层并非瓶颈，应到 dispatcher / provider 层继续排查。
    #[tokio::test]
    async fn test_task_list_create_first_call_is_fast() {
        use std::time::Instant;

        let store = Arc::new(TaskStore::new());
        let tool = TaskListCreateTool {
            store: store.clone(),
        };

        let first_start = Instant::now();
        let first = tool
            .call(
                serde_json::json!({"subject": "首次", "summary": "首次请求"}),
                &test_ctx(),
            )
            .await;
        let first_elapsed = first_start.elapsed();

        assert!(!first.is_error, "首次调用应成功，实际：{}", first.output);

        // 完成首个 batch 全部任务以允许新建第二个 batch
        store.complete_list().await;

        let second_start = Instant::now();
        let second = tool
            .call(
                serde_json::json!({"subject": "第二次", "summary": "第二次请求"}),
                &test_ctx(),
            )
            .await;
        let second_elapsed = second_start.elapsed();

        assert!(
            !second.is_error,
            "第二次调用应成功，实际：{}",
            second.output
        );

        // 工具层首次调用应远低于 dispatcher 层 120s 超时；用 1s 作为宽松阈值。
        assert!(
            first_elapsed.as_secs_f64() < 1.0,
            "首次调用耗时 {:?} 超过 1s，疑似工具层存在阻塞",
            first_elapsed
        );
        assert!(
            second_elapsed.as_secs_f64() < 1.0,
            "第二次调用耗时 {:?} 超过 1s",
            second_elapsed
        );
    }

    /// 复现 bug #70 的另一路径：存在 archived batch 时 create_list 会触发
    /// `drop_archived_batch_tasks`，验证该路径不会死锁/长阻塞。
    #[tokio::test]
    async fn test_task_list_create_with_archived_batch_is_fast() {
        use std::time::Instant;

        let store = Arc::new(TaskStore::new());
        let tool = TaskListCreateTool {
            store: store.clone(),
        };

        // 创建第一个 batch 并归档（直接 complete_list，无需任务）
        tool.call(
            serde_json::json!({"subject": "first", "summary": "first"}),
            &test_ctx(),
        )
        .await;
        store.complete_list().await;
        // 手动把 batch 标为 Archived 以触发 drop_archived_batch_tasks
        // （complete_list 已经设为 Archived）

        // 再次创建以触发 drop_archived_batch_tasks
        let start = Instant::now();
        let result = tool
            .call(
                serde_json::json!({"subject": "second", "summary": "second"}),
                &test_ctx(),
            )
            .await;
        let elapsed = start.elapsed();

        assert!(!result.is_error);
        assert!(
            elapsed.as_secs_f64() < 1.0,
            "带 archived batch 调用耗时 {:?} 超过 1s",
            elapsed
        );
    }
}

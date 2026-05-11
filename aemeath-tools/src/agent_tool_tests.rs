use super::*;
use aemeath_core::tool::{AgentRunner, ToolRegistry};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct StubRunner;

#[async_trait::async_trait]
impl AgentRunner for StubRunner {
    async fn run_agent(
        &self,
        prompt: &str,
        _system: &str,
        _tool_schemas: &[serde_json::Value],
        _registry: &ToolRegistry,
        _ctx: &ToolContext,
        _max_turns: Option<u32>,
        _model_spec: Option<&str>,
        _progress_tx: Option<tokio::sync::mpsc::Sender<aemeath_core::tool::AgentProgressEvent>>,
    ) -> String {
        prompt.to_string()
    }

    async fn complete(&self, prompt: &str, _system: &str, _ctx: &ToolContext) -> String {
        prompt.to_string()
    }
}

fn test_ctx() -> ToolContext {
    ToolContext {
        cwd: PathBuf::from("."),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        agent_runner: Some(Arc::new(StubRunner)),
        session_reminders: None,
        plan_mode: None,
        allow_all: false,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    }
}

#[tokio::test]
async fn test_agent_tool_task_id_success_completes_task() {
    let store = Arc::new(TaskStore::new());
    let task = store
        .create("agent task".to_string(), "run subagent".to_string(), None)
        .await;
    let tool = AgentTool {
        store: store.clone(),
    };

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
                "taskId": task.id,
            }),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error);
    let updated = store.get(&task.id).await.expect("task exists");
    assert_eq!(updated.status, TaskStatus::Completed);
}

#[tokio::test]
async fn test_agent_tool_task_id_failure_resets_pending() {
    let store = Arc::new(TaskStore::new());
    let task = store
        .create("agent task".to_string(), "run subagent".to_string(), None)
        .await;
    let tool = AgentTool {
        store: store.clone(),
    };

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "Sub-agent error: failed",
                "description": "run task",
                "taskId": task.id,
            }),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error);
    let updated = store.get(&task.id).await.expect("task exists");
    assert_eq!(updated.status, TaskStatus::Pending);
}

#[tokio::test]
async fn test_agent_tool_task_id_missing_task_errors() {
    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
                "taskId": "missing",
            }),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.output.contains("task not found"));
}

#[test]
fn test_analyze_task_scope_numbered_list_has_no_warning() {
    let prompt =
        "请按以下步骤执行：\n1. 读取文件\n2. 分析问题\n3. 修改代码\n4. 运行测试\n5. 汇报结果";

    let result = analyze_task_scope(prompt, &PathBuf::from("."));

    assert_eq!(result.level, ScopeLevel::Ok);
    assert!(result.warnings.is_empty());
}

#[test]
fn test_analyze_task_scope_large_task_pattern_still_blocks() {
    let prompt = "review all files in the entire codebase";

    let result = analyze_task_scope(prompt, &PathBuf::from("."));

    assert_eq!(result.level, ScopeLevel::Block);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("entire codebase")));
}

#[test]
fn test_analyze_task_scope_simple_task_still_warns() {
    let prompt = "read the file and summarize it";

    let result = analyze_task_scope(prompt, &PathBuf::from("."));

    assert_eq!(result.level, ScopeLevel::Warn);
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("simple task")));
}

#[test]
fn test_is_agent_failure_detects_known_markers() {
    assert!(is_agent_failure("Cancelled by user"));
    assert!(is_agent_failure(
        "Some text\n\n[Sub-agent timed out after 600s]"
    ));
    assert!(is_agent_failure("Sub-agent error: connection refused"));
    assert!(is_agent_failure(
        "Done\n\n[Sub-agent reached max turns (50)]"
    ));
}

#[test]
fn test_is_agent_failure_normal_result_is_not_failure() {
    assert!(!is_agent_failure("Successfully refactored the module."));
    assert!(!is_agent_failure(""));
    assert!(!is_agent_failure("No issues found in the reviewed files."));
}

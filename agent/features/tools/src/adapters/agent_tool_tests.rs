use super::*;
use crate::domain::{AgentRunRequest, AgentRunTerminal, AgentRunner};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct StubRunner {
    captured_timeout: Mutex<std::time::Duration>,
    captured_system: Mutex<String>,
    captured_parent_run_id: Mutex<Option<String>>,
    run_count: Mutex<usize>,
}

#[async_trait::async_trait]
impl AgentRunner for StubRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> AgentRunTerminal {
        *self.captured_timeout.lock().unwrap() = request.timeout;
        *self.captured_system.lock().unwrap() = request.system.to_string();
        *self.captured_parent_run_id.lock().unwrap() = Some(request.identity.run_id().to_string());
        *self.run_count.lock().unwrap() += 1;
        AgentRunTerminal::Completed {
            result: request.prompt.to_string(),
        }
    }

    async fn complete(
        &self,
        prompt: &str,
        _system: &str,
        _cancellation: Arc<dyn crate::domain::CancellationSignal>,
    ) -> String {
        prompt.to_string()
    }
}

fn test_ctx_with_runner(runner: Arc<dyn AgentRunner>) -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .agent(runner)
        .build()
}

fn test_ctx() -> ToolExecutionContext {
    test_ctx_with_runner(Arc::new(StubRunner::default()))
}

#[tokio::test]
async fn test_agent_tool_uses_finite_default_timeout() {
    let tool = AgentTool;
    let runner = Arc::new(StubRunner::default());
    let ctx = test_ctx_with_runner(runner.clone());

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(
        *runner.captured_timeout.lock().unwrap(),
        std::time::Duration::from_secs(1800)
    );
    assert!(runner
        .captured_system
        .lock()
        .unwrap()
        .contains("wall-clock timeout: 1800 seconds"));
}

#[tokio::test]
async fn test_agent_tool_caps_timeout_at_three_hours() {
    let tool = AgentTool;
    let runner = Arc::new(StubRunner::default());
    let ctx = test_ctx_with_runner(runner.clone());

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
                "timeout": 20000,
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(
        *runner.captured_timeout.lock().unwrap(),
        std::time::Duration::from_secs(10800)
    );
}

#[test]
fn test_agent_tool_schema_describes_timeout_without_max_turns() {
    let tool = AgentTool;

    let schema = tool.input_schema().to_string();
    let description = tool.description();

    assert!(schema.contains("timeout"));
    assert!(!schema.contains("max_turns"));
    assert!(!description.contains("1000 rounds"));
}

#[tokio::test]
async fn test_agent_tool_passes_parent_run_id_to_sub_agent_request() {
    let tool = AgentTool;
    let runner = Arc::new(StubRunner::default());
    let ctx = test_ctx_with_runner(runner.clone());

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(
        runner.captured_parent_run_id.lock().unwrap().as_ref(),
        Some(&ctx.scope().run_id().to_string())
    );
}

#[tokio::test]
async fn test_agent_tool_runs_without_task_id() {
    let tool = AgentTool;
    let runner = Arc::new(StubRunner::default());
    let ctx = test_ctx_with_runner(runner.clone());

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "finished",
                "description": "run task",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(*runner.run_count.lock().unwrap(), 1);
}

/// Regression: a sub-agent workspace is an isolated derivative of the parent:
/// it inherits the current location but subsequent child mutations cannot affect the parent.
#[test]
fn sub_agent_workspace_isolated() {
    use project::WorkspaceError;

    let main_dir = tempfile::tempdir().unwrap();
    let child_dir = main_dir.path().join("child");
    std::fs::create_dir_all(&child_dir).unwrap();
    let parent = project::wire_production_workspace(main_dir.path().to_path_buf())
        .expect("workspace initialization")
        .into_views();
    parent
        .control()
        .change_directory(child_dir.clone())
        .expect("change parent directory");

    let child = parent.derive_isolated();
    let canonical_main = main_dir.path().canonicalize().unwrap();
    let canonical_child = child_dir.canonicalize().unwrap();
    assert_eq!(child.read().current_path_base(), canonical_child);
    assert_eq!(child.read().current_workspace_root(), canonical_main);

    child
        .control()
        .change_directory(main_dir.path().to_path_buf())
        .expect("change child directory");
    assert_eq!(child.read().current_path_base(), canonical_main);
    assert_eq!(parent.read().current_path_base(), canonical_child);
    assert_eq!(
        child.control().exit(),
        Err(WorkspaceError::UnsupportedForNonGit)
    );
}

// ── #479 回归：text 字段必须包含子代理实际产出 ──

/// 子代理有产出时，text 必须等于产出（父 LLM 能看到）。
#[tokio::test]
async fn test_agent_tool_text_contains_subagent_output() {
    let tool = AgentTool;
    let ctx = test_ctx(); // StubRunner 返回 prompt 作为 output

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "这是子代理的实际产出内容",
                "description": "run task",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(result.text, "这是子代理的实际产出内容");
}

/// 子代理产出为空时，text 降级为合理的 summary。
#[tokio::test]
async fn test_agent_tool_text_fallback_when_output_empty() {
    // 用一个返回空串的 runner
    struct EmptyRunner;
    #[async_trait::async_trait]
    impl AgentRunner for EmptyRunner {
        async fn run_agent(&self, _request: AgentRunRequest<'_>) -> AgentRunTerminal {
            AgentRunTerminal::Completed {
                result: String::new(),
            }
        }
        async fn complete(
            &self,
            _prompt: &str,
            _system: &str,
            _cancellation: Arc<dyn crate::domain::CancellationSignal>,
        ) -> String {
            String::new()
        }
    }

    let tool = AgentTool;
    let ctx = test_ctx_with_runner(Arc::new(EmptyRunner));

    let result = tool
        .call(
            serde_json::json!({
                "prompt": "anything",
                "description": "run task",
            }),
            &ctx,
        )
        .await;

    assert!(!result.is_error);
    assert_eq!(result.text, "子代理执行完成（无输出）");
}

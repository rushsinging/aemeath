use super::*;
use crate::api::{AgentRunRequest, AgentRunTerminal, AgentRunner, ToolResources};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

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
        *self.captured_parent_run_id.lock().unwrap() = Some(request.ctx.run_id.clone());
        *self.run_count.lock().unwrap() += 1;
        AgentRunTerminal::Completed {
            result: request.prompt.to_string(),
        }
    }

    async fn complete(&self, prompt: &str, _system: &str, _ctx: &ToolExecutionContext) -> String {
        prompt.to_string()
    }
}

fn test_ctx_with_runner(runner: Arc<dyn AgentRunner>) -> ToolExecutionContext {
    ToolExecutionContext {
        workspace: project::wire_production_workspace(PathBuf::from(".")).into_views(),
        run_id: "01900000-0000-7000-8000-000000000001".to_string(),
        cancel: CancellationToken::new(),
        read_files: Arc::new(Mutex::new(HashSet::new())),
        resources: ToolResources {
            agent_runner: Some(runner),
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: false,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    }
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
        Some(&ctx.run_id)
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

/// 回归：子 agent 的 workspace 必须从父快照派生为独立实例（继承位置、空栈、独立 Arc/锁），
/// 子的 worktree 进出不得影响父（修隔离 bug）。
#[test]
fn sub_agent_workspace_isolated() {
    use project::WorkspaceError;
    use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

    // 用真实存在的临时目录满足 restore 的路径校验。
    let main_dir = tempfile::tempdir().unwrap();
    let wt_dir = tempfile::tempdir().unwrap();

    // 父：进入一个伪 worktree（path_base/workspace_root = wt，栈里压入 main 帧）。
    let parent = project::wire_production_workspace(main_dir.path().to_path_buf()).into_views();
    let dto = PersistedWorkspaceContext {
        path_base: wt_dir.path().display().to_string(),
        workspace_root: wt_dir.path().display().to_string(),
        context_stack: vec![PersistedWorkspaceFrame {
            path_base: main_dir.path().display().to_string(),
            workspace_root: main_dir.path().display().to_string(),
        }],
    };
    parent
        .persist()
        .restore(&dto)
        .expect("restore parent workspace");

    // 子：从父快照派生。
    let child = parent.derive_isolated();

    // 1) 子继承父当前位置，但后续变更隔离。
    assert_eq!(child.read().current_path_base(), wt_dir.path());
    assert_eq!(child.read().current_workspace_root(), wt_dir.path());

    // 2) 子栈独立为空：exit 报 EmptyStack。
    assert_eq!(child.control().exit(), Err(WorkspaceError::EmptyStack));

    // 3) 父栈不受子影响：父仍可 exit 回到 main。
    let prev = parent.control().exit().expect("parent still has a frame");
    assert_eq!(prev.path_base, main_dir.path());
    assert_eq!(parent.read().current_path_base(), main_dir.path());
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
            _ctx: &ToolExecutionContext,
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

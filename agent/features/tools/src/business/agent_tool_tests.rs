use super::*;
use crate::api::{AgentRunRequest, AgentRunTerminal, AgentRunner, ToolResources};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct StubRunner {
    captured_timeout: Mutex<std::time::Duration>,
    captured_system: Mutex<String>,
    run_count: Mutex<usize>,
}

#[async_trait::async_trait]
impl AgentRunner for StubRunner {
    async fn run_agent(&self, request: AgentRunRequest<'_>) -> AgentRunTerminal {
        *self.captured_timeout.lock().unwrap() = request.timeout;
        *self.captured_system.lock().unwrap() = request.system.to_string();
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
        workspace: project::api::WorkspaceService::new(PathBuf::from(".")),
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
    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };
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
    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };
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
    let tool = AgentTool {
        store: Arc::new(TaskStore::new()),
    };

    let schema = tool.input_schema().to_string();
    let description = tool.description();

    assert!(schema.contains("timeout"));
    assert!(!schema.contains("max_turns"));
    assert!(!description.contains("1000 rounds"));
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
                "task_id": task.id,
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
    struct FailedRunner;
    #[async_trait::async_trait]
    impl AgentRunner for FailedRunner {
        async fn run_agent(&self, _request: AgentRunRequest<'_>) -> AgentRunTerminal {
            AgentRunTerminal::Failed {
                error: "failed".to_string(),
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
                "prompt": "ordinary user data",
                "description": "run task",
                "task_id": task.id,
            }),
            &test_ctx_with_runner(Arc::new(FailedRunner)),
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
                "task_id": "missing",
            }),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("task not found"));
}

#[tokio::test]
async fn test_agent_tool_allows_missing_task_id_even_with_active_list() {
    let store = Arc::new(TaskStore::new());
    store
        .create_list("request".to_string(), "complex request".to_string())
        .await;
    store
        .create("agent task".to_string(), "run subagent".to_string(), None)
        .await;
    let tool = AgentTool {
        store: store.clone(),
    };
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

    // Free-form agent calls without taskId should succeed even when an active
    // task list has incomplete tasks.
    assert!(!result.is_error);
    assert_eq!(*runner.run_count.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_agent_tool_allows_missing_task_id_without_active_list() {
    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };
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

#[test]
fn test_is_agent_failure_detects_known_markers() {
    assert!(is_agent_failure(&AgentRunTerminal::Cancelled));
    assert!(is_agent_failure(&AgentRunTerminal::Failed {
        error: "connection refused".to_string(),
    }));
}

#[test]
fn test_is_agent_failure_normal_result_is_not_failure() {
    assert!(!is_agent_failure(&AgentRunTerminal::Completed {
        result: "Successfully refactored the module.".to_string(),
    }));
    assert!(!is_agent_failure(&AgentRunTerminal::Completed {
        result: String::new(),
    }));
}

/// 回归：子 agent 的 workspace 必须从父快照派生为独立实例（继承位置、空栈、独立 Arc/锁），
/// 子的 worktree 进出不得影响父（修隔离 bug）。
#[test]
fn sub_agent_workspace_isolated() {
    use project::api::{WorkspaceControl, WorkspaceError, WorkspacePersist, WorkspaceRead};
    use share::session_types::{PersistedWorkspaceContext, PersistedWorkspaceFrame};

    // 用真实存在的临时目录满足 restore 的路径校验。
    let main_dir = tempfile::tempdir().unwrap();
    let wt_dir = tempfile::tempdir().unwrap();

    // 父：进入一个伪 worktree（path_base/workspace_root = wt，栈里压入 main 帧）。
    let parent = project::api::WorkspaceService::new(main_dir.path().to_path_buf());
    let dto = PersistedWorkspaceContext {
        path_base: wt_dir.path().display().to_string(),
        workspace_root: wt_dir.path().display().to_string(),
        context_stack: vec![PersistedWorkspaceFrame {
            path_base: main_dir.path().display().to_string(),
            workspace_root: main_dir.path().display().to_string(),
        }],
    };
    WorkspacePersist::restore(parent.as_ref(), &dto).expect("restore parent workspace");

    // 子：从父快照派生。
    let child = parent.seed_isolated();

    // 1) 不是同一个 Arc。
    assert!(
        !Arc::ptr_eq(&parent, &child),
        "child workspace 必须是独立 Arc 实例"
    );

    // 2) 子继承父当前位置。
    assert_eq!(child.current_path_base(), wt_dir.path());
    assert_eq!(child.current_workspace_root(), wt_dir.path());

    // 3) 子栈独立为空：exit 报 EmptyStack。
    assert_eq!(
        WorkspaceControl::exit(child.as_ref()),
        Err(WorkspaceError::EmptyStack)
    );

    // 4) 父栈不受子影响：父仍可 exit 回到 main。
    let prev = WorkspaceControl::exit(parent.as_ref()).expect("parent still has a frame");
    assert_eq!(prev.path_base, main_dir.path());
    assert_eq!(parent.current_path_base(), main_dir.path());
}

// ── #479 回归：text 字段必须包含子代理实际产出 ──

/// 子代理有产出时，text 必须等于产出（父 LLM 能看到）。
#[tokio::test]
async fn test_agent_tool_text_contains_subagent_output() {
    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };
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

    let store = Arc::new(TaskStore::new());
    let tool = AgentTool { store };
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

use super::*;
use crate::application::loop_engine::chat::committed_side_effect::{
    CommittedSideEffectDispatcher, TaskCommittedSideEffectHandler,
};

#[test]
fn tool_forwards_progress_stream_includes_bash_and_watch_only() {
    assert!(tool_forwards_progress_stream("Bash"));
    assert!(tool_forwards_progress_stream("BackgroundProcessWatch"));
    assert!(!tool_forwards_progress_stream("BackgroundProcessStatus"));
    assert!(!tool_forwards_progress_stream("BackgroundProcessLogs"));
    assert!(!tool_forwards_progress_stream("Read"));
}
use crate::application::loop_engine::chat::{ChatEventSinkHandle, EventFuture};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;
use task::{BatchCreateSpecData, TaskAccess, TaskCreateSpecData, TaskPriorityData, TaskStatusData};
use tools::published::execution::ToolExecutionContext;
use tools::published::typed::{TypedTool, TypedToolResult};

struct ConcurrencyFlagTool {
    name: &'static str,
    safe: bool,
}

#[async_trait]
impl TypedTool for ConcurrencyFlagTool {
    type Output = Value;

    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "concurrency classification test tool"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type": "object"})
    }

    fn is_concurrency_safe(&self) -> bool {
        self.safe
    }

    async fn call(
        &self,
        _input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        TypedToolResult::success("ok", Value::Null)
    }
}

fn test_ctx() -> ToolExecutionContext {
    crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    )
}

fn call(name: &str, index: usize) -> ToolCall {
    ToolCall {
        provider_id: "provider-test".to_string(),
        id: sdk::ids::ToolCallId::from_legacy_or_new(&format!("call-{index}")),
        name: name.to_string(),
        index,
        input: serde_json::json!({}),
    }
}

#[test]
fn test_partition_calls_routes_concurrency_safe_tools_to_concurrent() {
    let registry = tools::composition::TestCatalogExecutionFactory::new();
    registry.register(ConcurrencyFlagTool {
        name: "safe_a",
        safe: true,
    });
    registry.register(ConcurrencyFlagTool {
        name: "safe_b",
        safe: true,
    });
    let agent = Agent::for_test(&registry, test_ctx(), 10);
    let calls = [call("safe_a", 0), call("safe_b", 1)];
    let prepared = calls
        .into_iter()
        .map(|call| PreparedToolCall {
            call,
            authorization: share::tools_vocab::AuthorizationContext::STANDARD,
        })
        .collect::<Vec<_>>();
    let refs = prepared.iter().collect::<Vec<_>>();

    let (concurrent, sequential) = partition_calls(&agent, &refs);

    assert_eq!(concurrent, vec![0, 1]);
    assert!(sequential.is_empty());
}

#[test]
fn test_partition_calls_routes_non_concurrency_safe_tools_to_sequential() {
    let registry = tools::composition::TestCatalogExecutionFactory::new();
    registry.register(ConcurrencyFlagTool {
        name: "unsafe_a",
        safe: false,
    });
    registry.register(ConcurrencyFlagTool {
        name: "unsafe_b",
        safe: false,
    });
    let agent = Agent::for_test(&registry, test_ctx(), 10);
    let calls = [call("unsafe_a", 0), call("unsafe_b", 1)];
    let prepared = calls
        .into_iter()
        .map(|call| PreparedToolCall {
            call,
            authorization: share::tools_vocab::AuthorizationContext::STANDARD,
        })
        .collect::<Vec<_>>();
    let refs = prepared.iter().collect::<Vec<_>>();

    let (concurrent, sequential) = partition_calls(&agent, &refs);

    assert!(concurrent.is_empty());
    assert_eq!(sequential, vec![0, 1]);
}

#[test]
fn test_partition_calls_preserves_mixed_positions() {
    let registry = tools::composition::TestCatalogExecutionFactory::new();
    registry.register(ConcurrencyFlagTool {
        name: "safe",
        safe: true,
    });
    registry.register(ConcurrencyFlagTool {
        name: "unsafe",
        safe: false,
    });
    let agent = Agent::for_test(&registry, test_ctx(), 10);
    let calls = [call("safe", 0), call("unsafe", 1), call("safe", 2)];
    let prepared = calls
        .into_iter()
        .map(|call| PreparedToolCall {
            call,
            authorization: share::tools_vocab::AuthorizationContext::STANDARD,
        })
        .collect::<Vec<_>>();
    let refs = prepared.iter().collect::<Vec<_>>();

    let (concurrent, sequential) = partition_calls(&agent, &refs);

    assert_eq!(concurrent, vec![0, 2]);
    assert_eq!(sequential, vec![1]);
}

#[test]
fn test_partition_calls_routes_unknown_tools_to_sequential() {
    let registry = tools::composition::TestCatalogExecutionFactory::new();
    let agent = Agent::for_test(&registry, test_ctx(), 10);
    let calls = [call("missing", 0)];
    let prepared = calls
        .into_iter()
        .map(|call| PreparedToolCall {
            call,
            authorization: share::tools_vocab::AuthorizationContext::STANDARD,
        })
        .collect::<Vec<_>>();
    let refs = prepared.iter().collect::<Vec<_>>();

    let (concurrent, sequential) = partition_calls(&agent, &refs);

    assert!(concurrent.is_empty());
    assert_eq!(sequential, vec![0]);
}

#[derive(Default)]
struct RecordingTaskHook {
    invocations: Mutex<Vec<&'static str>>,
}

#[async_trait]
impl HookDispatcher for RecordingTaskHook {
    async fn dispatch(
        &self,
        invocation: HookInvocationData,
        _cancellation: &dyn hook::HookCancellationSignal,
    ) -> hook::HookOutcomeData {
        let kind = match invocation {
            HookInvocationData::TaskCreated { .. } => Some("created"),
            HookInvocationData::TaskCompleted { .. } => Some("completed"),
            _ => None,
        };
        if let Some(kind) = kind {
            self.invocations.lock().unwrap().push(kind);
        }
        hook::HookOutcomeData::proceed()
    }
}

fn created_task_outcome(text: &str) -> tools::ToolOutcome {
    let store = task::TaskStore::new();
    store
        .create_batch(BatchCreateSpecData::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let result = store
        .create_task(
            TaskCreateSpecData::try_new(
                "subject".to_owned(),
                String::new(),
                None,
                TaskPriorityData::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    tools::ToolOutcome::new(text, Value::Null, Vec::new())
        .with_task_change(tools::CommittedTaskChange::from_command_result(&result))
}

fn completed_task_outcome(text: &str) -> tools::ToolOutcome {
    let store = task::TaskStore::new();
    store
        .create_batch(BatchCreateSpecData::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store
        .create_task(
            TaskCreateSpecData::try_new(
                "subject".to_owned(),
                String::new(),
                None,
                TaskPriorityData::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let result = store
        .transition_with_progress(created.value.id(), TaskStatusData::Completed, 3)
        .unwrap();
    tools::ToolOutcome::new(text, Value::Null, Vec::new())
        .with_task_change(tools::CommittedTaskChange::from_command_result(&result))
}

#[derive(Clone)]
struct NoopTaskEventSink;

impl ChatEventSink for NoopTaskEventSink {
    fn send_event<'a>(&'a self, _event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, _event: RuntimeStreamEvent) {}
}

async fn dispatch_task_facts(outcome: tools::ToolOutcome) -> Vec<&'static str> {
    let hook = Arc::new(RecordingTaskHook::default());
    let hook_port: Arc<dyn HookDispatcher> = hook.clone();
    let store = Arc::new(task::TaskStore::new());
    let revision = outcome
        .task_change
        .as_ref()
        .map(|change| change.revision().get());
    let dispatcher =
        CommittedSideEffectDispatcher::new(vec![Arc::new(TaskCommittedSideEffectHandler::new(
            store.clone(),
            ChatEventSinkHandle::new(NoopTaskEventSink),
            "session-test".to_owned(),
            hook_port,
            Arc::new(ActivityCoordinator::production_without_publisher(
                sdk::RunId::new_v7(),
                crate::application::activity::RunPurpose::Main,
            )),
            std::env::current_dir().unwrap(),
        ))]);
    let tool_call = call("ordinary", 0);
    let execution = crate::application::tool::agent::ToolExecution::new(&tool_call, outcome);
    if let Some(revision) = revision {
        store
            .create_batch(BatchCreateSpecData::try_new("batch".to_owned()).unwrap(), 1)
            .unwrap();
        if revision >= 2 {
            store
                .create_task(
                    TaskCreateSpecData::try_new(
                        "subject".to_owned(),
                        String::new(),
                        None,
                        TaskPriorityData::Normal,
                    )
                    .unwrap(),
                    2,
                )
                .unwrap();
        }
        if revision >= 3 {
            let task_id = store.list()[0].id();
            store
                .transition_with_progress(task_id, TaskStatusData::Completed, 3)
                .unwrap();
        }
    }
    dispatcher
        .observe(
            &tool_call,
            &execution,
            &sdk::RunStepId::new_v7(),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
    let invocations = hook.invocations.lock().unwrap().clone();
    invocations
}

#[tokio::test]
async fn typed_task_facts_dispatch_created_and_completed_hooks() {
    let created = dispatch_task_facts(created_task_outcome("created")).await;
    let completed = dispatch_task_facts(completed_task_outcome("completed")).await;

    assert_eq!(created, vec!["created"]);
    assert_eq!(completed, vec!["completed"]);
}

#[tokio::test]
async fn missing_committed_change_never_infers_task_hooks_from_name_or_result_text() {
    let misleading_result = ["Status", ": ", "Completed"].concat();
    let outcome = tools::ToolOutcome::new(misleading_result, Value::Null, Vec::new());

    assert!(dispatch_task_facts(outcome).await.is_empty());
}

// ── progress 工具族 goal 注入（#2113）───────────────────────────

/// 账本 fake：`bgp_ledger` 有 goal，`bgp_blank` goal 为空，其余未知。
struct GoalLedgerAccess;

impl tools::BackgroundProcessAccess for GoalLedgerAccess {
    fn list_tasks(&self) -> Vec<tools::types::background_processes::BackgroundProcessSummaryData> {
        vec![]
    }

    fn task_status(
        &self,
        task_id: &str,
    ) -> Option<tools::types::background_processes::BackgroundProcessDetailData> {
        let goal = match task_id {
            "bgp_ledger" => "cargo test 全量门禁",
            "bgp_blank" => "",
            _ => return None,
        };
        Some(
            tools::types::background_processes::BackgroundProcessDetailData {
                summary: tools::types::background_processes::BackgroundProcessSummaryData {
                    task_id: task_id.to_string(),
                    tool_name: "Bash".to_string(),
                    state: "backgrounded".to_string(),
                    goal: goal.to_string(),
                    duration_ms: None,
                },
                deadline_remaining_ms: None,
                total_written_bytes: 0,
            },
        )
    }

    fn read_task_log(
        &self,
        _task_id: &str,
        _cursor: Option<u64>,
        _max_bytes: usize,
    ) -> Option<tools::types::background_processes::BackgroundProcessLogData> {
        None
    }

    fn stop_task(
        &self,
        task_id: &str,
    ) -> Result<tools::types::background_processes::BackgroundProcessStopData, String> {
        Err(format!("unknown task {task_id}"))
    }
}

#[test]
fn goal_enrichable_tools_cover_all_task_id_progress_tools() {
    for tool_name in [
        "BackgroundProcessWatch",
        "BackgroundProcessStatus",
        "BackgroundProcessLogs",
        "BackgroundProcessStop",
    ] {
        assert!(
            is_goal_enrichable_tool(tool_name),
            "{tool_name} 应注入 goal"
        );
    }
    for tool_name in ["BackgroundProcessList", "Bash", "Read", "TaskGet"] {
        assert!(
            !is_goal_enrichable_tool(tool_name),
            "{tool_name} NEVER 注入 goal"
        );
    }
}

#[test]
fn ledger_goal_injected_for_every_progress_tool_with_task_id() {
    for tool_name in [
        "BackgroundProcessWatch",
        "BackgroundProcessStatus",
        "BackgroundProcessLogs",
        "BackgroundProcessStop",
    ] {
        let mut progress_call = call(tool_name, 1);
        progress_call.input = serde_json::json!({"task_id": "bgp_ledger"});
        inject_goal_from_ledger(&mut progress_call, &GoalLedgerAccess);
        assert_eq!(
            progress_call.input.get("goal"),
            Some(&Value::String("cargo test 全量门禁".to_string())),
            "{tool_name} header 映射依赖注入的 goal"
        );
    }
}

#[test]
fn ledger_goal_never_injected_for_unknown_id_blank_goal_or_missing_task_id() {
    let mut unknown_id_call = call("BackgroundProcessStatus", 2);
    unknown_id_call.input = serde_json::json!({"task_id": "bgp_missing"});
    inject_goal_from_ledger(&mut unknown_id_call, &GoalLedgerAccess);
    assert!(
        unknown_id_call.input.get("goal").is_none(),
        "未知 id NEVER 注入 goal"
    );

    let mut blank_goal_call = call("BackgroundProcessStop", 3);
    blank_goal_call.input = serde_json::json!({"task_id": "bgp_blank"});
    inject_goal_from_ledger(&mut blank_goal_call, &GoalLedgerAccess);
    assert!(
        blank_goal_call.input.get("goal").is_none(),
        "空 goal NEVER 注入"
    );

    let mut no_task_id_call = call("BackgroundProcessLogs", 4);
    no_task_id_call.input = serde_json::json!({"timeout": 60});
    inject_goal_from_ledger(&mut no_task_id_call, &GoalLedgerAccess);
    assert!(
        no_task_id_call.input.get("goal").is_none(),
        "无 task_id NEVER 注入 goal"
    );
}

use super::*;
use crate::application::loop_engine::chat::committed_side_effect::{
    CommittedSideEffectDispatcher, TaskCommittedSideEffectHandler,
};
use crate::application::loop_engine::chat::{ChatEventSinkHandle, EventFuture};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskStatus};
use tools::{ToolExecutionContext, TypedTool, TypedToolResult};

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
            authorization: tools::AuthorizationContext::STANDARD,
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
            authorization: tools::AuthorizationContext::STANDARD,
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
            authorization: tools::AuthorizationContext::STANDARD,
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
            authorization: tools::AuthorizationContext::STANDARD,
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
impl HookPort for RecordingTaskHook {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> hook::HookOutcome {
        let kind = match invocation {
            HookInvocation::TaskCreated(_) => Some("created"),
            HookInvocation::TaskCompleted(_) => Some("completed"),
            _ => None,
        };
        if let Some(kind) = kind {
            self.invocations.lock().unwrap().push(kind);
        }
        hook::HookOutcome::proceed()
    }
}

fn created_task_outcome(text: &str) -> tools::ToolOutcome {
    let store = task::TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let result = store
        .create_task(
            TaskCreateSpec::try_new(
                "subject".to_owned(),
                String::new(),
                None,
                TaskPriority::Normal,
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
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let created = store
        .create_task(
            TaskCreateSpec::try_new(
                "subject".to_owned(),
                String::new(),
                None,
                TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let result = store
        .transition(created.value.id(), TaskStatus::Completed, 3)
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
    let hook_port: Arc<dyn HookPort> = hook.clone();
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
            )),
            std::env::current_dir().unwrap(),
        ))]);
    let tool_call = call("ordinary", 0);
    let execution = crate::application::tool::agent::ToolExecution::new(&tool_call, outcome);
    if let Some(revision) = revision {
        store
            .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
            .unwrap();
        if revision >= 2 {
            store
                .create_task(
                    TaskCreateSpec::try_new(
                        "subject".to_owned(),
                        String::new(),
                        None,
                        TaskPriority::Normal,
                    )
                    .unwrap(),
                    2,
                )
                .unwrap();
        }
        if revision >= 3 {
            let task_id = store.list()[0].id();
            store.transition(task_id, TaskStatus::Completed, 3).unwrap();
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

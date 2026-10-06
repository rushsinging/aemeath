use super::*;
use crate::application::loop_engine::chat::{EventFuture, RuntimeStreamEvent};
use async_trait::async_trait;
use sdk::ids::{ChatId, ChatRunId, ToolCallId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tokio::sync::{mpsc, Notify};
use tools::published::typed::{TypedTool, TypedToolResult};

#[test]
fn sub_run_activity_projection_preserves_parent_tool_identity_and_kinds() {
    let parent_context =
        RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
    let parent_tool_id = ToolCallId::new("agent-call");
    let event = tools::published::agent::AgentProgressEvent {
        source_context: Some(tools::published::agent::AgentProgressSourceContext::new(
            "researcher",
            "child-run",
        )),
        sequence: 7,
        kind: tools::published::agent::AgentProgressKind::Thinking {
            text: "reasoning".to_string(),
        },
    };

    let mut publisher = SubRunFactPublisher::new(parent_context.clone(), parent_tool_id.clone());
    let projected = publisher.publish(event);

    assert_eq!(projected.len(), 1);
    let SubRunPublishedFact::Activity(activity) = &projected[0] else {
        panic!("expected sub run activity");
    };
    assert_eq!(activity.identity.agent_id, "researcher");
    assert_eq!(activity.identity.run_id, "child-run");
    assert_eq!(
        activity.identity.parent_chat_id,
        parent_context.chat_id.to_string()
    );
    assert_eq!(
        activity.identity.parent_run_id,
        parent_context.run_id.to_string()
    );
    assert_eq!(
        activity.identity.spawned_by_tool_call_id,
        parent_tool_id.to_string()
    );
    assert_eq!(activity.sequence, 1);
    assert!(matches!(
        &activity.kind,
        tools::published::sub_run::SubRunActivityKind::Thinking { text } if text == "reasoning"
    ));
}

#[test]
fn sub_run_tool_result_preserves_canonical_tool_name() {
    let parent_context =
        RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
    let parent_tool_id = ToolCallId::new("agent-call");
    let mut publisher = SubRunFactPublisher::new(parent_context, parent_tool_id);
    let started = tools::published::agent::AgentProgressEvent {
        source_context: Some(tools::published::agent::AgentProgressSourceContext::new(
            "researcher",
            "child-run",
        )),
        sequence: 0,
        kind: tools::published::agent::AgentProgressKind::Started {
            role: Some("researcher".to_string()),
            model: "model".to_string(),
        },
    };
    let SubRunPublishedFact::Started(started) = &publisher.publish(started)[0] else {
        panic!("expected sub run started");
    };
    assert_eq!(started.sequence, 1);

    let projected = publisher.publish(tools::published::agent::AgentProgressEvent {
        source_context: None,
        sequence: 1,
        kind: tools::published::agent::AgentProgressKind::ToolResult {
            tool_call_id: "skill-call".to_string(),
            tool_name: "Skill".to_string(),
            output: "SKILL_BODY_SENTINEL".to_string(),
            content: serde_json::json!({"name": "using-superpowers"}),
            is_error: false,
        },
    });

    let SubRunPublishedFact::Activity(activity) = &projected[0] else {
        panic!("expected sub run activity");
    };
    assert!(matches!(
        &activity.kind,
        tools::published::sub_run::SubRunActivityKind::ToolResult {
            tool_name,
            output,
            ..
        } if tool_name == "Skill" && output == "SKILL_BODY_SENTINEL"
    ));
}

#[test]
fn sub_run_activity_projection_sequences_tool_output_without_source_context() {
    let parent_context =
        RuntimeRunContext::new(ChatId::new("parent-chat"), ChatRunId::new("parent-run"));
    let parent_tool_id = ToolCallId::new("agent-call");
    let mut publisher = SubRunFactPublisher::new(parent_context, parent_tool_id);
    let started = tools::published::agent::AgentProgressEvent {
        source_context: Some(tools::published::agent::AgentProgressSourceContext::new(
            "researcher",
            "child-run",
        )),
        sequence: 0,
        kind: tools::published::agent::AgentProgressKind::Started {
            role: Some("researcher".to_string()),
            model: "model".to_string(),
        },
    };
    let SubRunPublishedFact::Started(started) = &publisher.publish(started)[0] else {
        panic!("expected sub run started");
    };
    assert_eq!(started.sequence, 1);

    let output = publisher.publish(tools::published::agent::AgentProgressEvent {
        source_context: None,
        sequence: 1,
        kind: tools::published::agent::AgentProgressKind::ToolOutput {
            tool_name: "Bash".to_string(),
            text: "hello".to_string(),
        },
    });

    assert_eq!(output.len(), 1);
    let SubRunPublishedFact::Activity(output) = &output[0] else {
        panic!("expected sub run activity");
    };
    assert_eq!(output.identity.run_id, "child-run");
    assert_eq!(output.sequence, 2);
    assert!(matches!(
        &output.kind,
        tools::published::sub_run::SubRunActivityKind::ToolOutput { tool_name, text }
            if tool_name == "Bash" && text == "hello"
    ));
}

#[derive(Clone)]
struct NoopSink;

impl ChatEventSink for NoopSink {
    fn send_event<'a>(&'a self, _event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async {})
    }

    fn try_send_event(&self, _event: RuntimeStreamEvent) {}
}

/// A test HookDispatcher that always returns Continue.
struct NoOpHookPort;

#[async_trait]
impl HookDispatcher for NoOpHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocationData,
        _cancellation: &dyn hook::HookCancellationSignal,
    ) -> hook::HookOutcomeData {
        hook::HookOutcomeData::proceed()
    }
}

struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ControlledAgentTool {
    started: mpsc::UnboundedSender<String>,
    gates: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

#[async_trait]
impl TypedTool for ControlledAgentTool {
    type Output = Value;

    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "controlled agent test tool"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object"})
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let label = input["label"].as_str().unwrap().to_string();
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        let _guard = ActiveGuard(self.active.clone());
        let gate = self.gates.lock().unwrap()[&label].clone();
        self.started.send(label.clone()).unwrap();
        gate.notified().await;
        TypedToolResult::success(label.clone(), serde_json::json!({"label": label}))
    }
}

struct Harness {
    execution: Arc<dyn ToolExecutionPort>,
    catalog: tools::ToolCatalogSnapshot,
    ctx: ToolExecutionContext,
    started: mpsc::UnboundedReceiver<String>,
    gates: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    max_active: Arc<AtomicUsize>,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
}

fn harness(labels: &[&str], limit: usize) -> Harness {
    let (started_tx, started) = mpsc::unbounded_channel();
    let gates = Arc::new(Mutex::new(
        labels
            .iter()
            .map(|label| ((*label).to_string(), Arc::new(Notify::new())))
            .collect(),
    ));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let factory = tools::composition::TestCatalogExecutionFactory::new();
    factory.register(ControlledAgentTool {
        started: started_tx,
        gates: gates.clone(),
        active,
        max_active: max_active.clone(),
    });
    let cwd = std::env::current_dir().unwrap();
    let ctx = crate::application::run::workspace_test_support::test_tool_execution_context(
        cwd,
        CancellationToken::new(),
    );
    let ports = factory.build(ctx.clone());
    let catalog = ports.catalog();
    Harness {
        execution: ports.execution(),
        catalog,
        ctx,
        started,
        gates,
        max_active,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(limit)),
    }
}

fn call(label: &str, index: usize) -> ToolCall {
    ToolCall {
        id: ToolCallId::from_legacy_or_new(&format!("call-{label}")),
        provider_id: format!("provider-{label}"),
        name: "Agent".to_string(),
        index,
        input: serde_json::json!({"label": label}),
    }
}

fn notify(gates: &Arc<Mutex<HashMap<String, Arc<Notify>>>>, label: &str) {
    gates.lock().unwrap()[label].notify_one();
}

fn spawn_calls(
    execution: Arc<dyn ToolExecutionPort>,
    ctx: ToolExecutionContext,
    calls: Vec<ToolCall>,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    cancel: CancellationToken,
    catalog: tools::ToolCatalogSnapshot,
) -> tokio::task::JoinHandle<Vec<ToolExecution>> {
    tokio::spawn(async move {
        let sink = NoopSink;
        let hook_port: Arc<dyn HookDispatcher> = Arc::new(NoOpHookPort);
        let activities = crate::application::activity::ActivityCoordinator::new(
            sdk::RunId::new_v7(),
            Arc::new(crate::application::activity::SystemActivityClock),
            Arc::new(crate::application::activity::UuidV7ActivityIdSource),
        );
        let prepared = calls
            .into_iter()
            .map(|call| PreparedToolCall {
                call,
                authorization: share::tools_vocab::AuthorizationContext::STANDARD,
            })
            .collect::<Vec<_>>();
        let agent = crate::application::tool::agent::Agent {
            catalog: catalog.clone(),
            execution,
            context: crate::application::context::coordination::ContextCoordinator::new(
                context::wire_isolated_context("test-session"),
            ),
            session_id: context::SessionId::new("test-session"),
            ctx: ctx.clone(),
            max_tool_concurrency: 1,
            agent_semaphore: agent_semaphore.clone(),
            workspace_persist: crate::application::run::workspace_test_support::workspace_persist(
                &ctx,
            ),
            tool_result_materializer:
                crate::application::tool::test_support::test_tool_result_materializer(),
            committed_side_effects: Default::default(),
            runtime_cancellation: cancel.clone(),
            background_threshold: None,
            background_tasks: None,
        };
        let step_tool_context = ctx.with_cancellation(Arc::new(
            crate::application::run::context::RunCancellationScope::from_token(cancel.clone()),
        ));
        execute_agent_calls(
            &RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn")),
            &prepared,
            &agent,
            &step_tool_context,
            &agent_semaphore,
            &crate::application::run::workspace_test_support::workspace_persist(&ctx),
            &sink,
            &hook_port,
            &activities,
            &cancel,
            &ctx.workspace_read(),
            &catalog,
            &*policy::allow_all(),
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
        )
        .await
    })
}

#[tokio::test]
async fn test_agent_window_starts_next_call_when_one_slot_frees() {
    let mut h = harness(&["first", "slow", "next"], 2);
    let handle = spawn_calls(
        h.execution.clone(),
        h.ctx.clone(),
        vec![call("first", 0), call("slow", 1), call("next", 2)],
        h.agent_semaphore.clone(),
        CancellationToken::new(),
        h.catalog.clone(),
    );

    let first_two = [
        h.started.recv().await.unwrap(),
        h.started.recv().await.unwrap(),
    ];
    assert!(first_two.contains(&"first".to_string()));
    assert!(first_two.contains(&"slow".to_string()));
    notify(&h.gates, "first");
    let next = tokio::time::timeout(std::time::Duration::from_secs(2), h.started.recv())
        .await
        .expect("next Agent should start as soon as one permit is free")
        .unwrap();
    assert_eq!(next, "next");

    notify(&h.gates, "slow");
    notify(&h.gates, "next");
    let results = handle.await.unwrap();
    assert_eq!(
        results
            .iter()
            .map(|result| result.provider_id.as_str())
            .collect::<Vec<_>>(),
        vec!["provider-first", "provider-slow", "provider-next"]
    );
    assert_eq!(h.max_active.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_agent_semaphore_is_shared_across_rounds() {
    let mut h = harness(&["one", "two"], 1);
    let first = spawn_calls(
        h.execution.clone(),
        h.ctx.clone(),
        vec![call("one", 0)],
        h.agent_semaphore.clone(),
        CancellationToken::new(),
        h.catalog.clone(),
    );
    assert_eq!(h.started.recv().await.unwrap(), "one");
    let second = spawn_calls(
        h.execution.clone(),
        h.ctx.clone(),
        vec![call("two", 0)],
        h.agent_semaphore.clone(),
        CancellationToken::new(),
        h.catalog.clone(),
    );

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), h.started.recv())
            .await
            .is_err(),
        "second round must wait for the shared Agent permit"
    );
    notify(&h.gates, "one");
    assert_eq!(h.started.recv().await.unwrap(), "two");
    notify(&h.gates, "two");
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(h.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn running_agent_call_uses_current_step_cancellation() {
    let mut harness = harness(&["running"], 1);
    let step_cancel = CancellationToken::new();
    let handle = spawn_calls(
        harness.execution.clone(),
        harness.ctx.clone(),
        vec![call("running", 0)],
        harness.agent_semaphore.clone(),
        step_cancel.clone(),
        harness.catalog.clone(),
    );
    assert_eq!(harness.started.recv().await.unwrap(), "running");

    step_cancel.cancel();
    let results = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("当前 Step 取消必须终止运行中的 Agent tool call")
        .unwrap();

    assert_eq!(results.len(), 1);
    assert!(results[0].outcome.is_error);
    assert!(
        results[0].outcome.text.contains("cancel"),
        "Agent tool terminal 必须保留取消语义: {}",
        results[0].outcome.text
    );
}

#[tokio::test]
async fn test_cancelled_agent_waiter_never_starts() {
    let mut h = harness(&["running", "waiting"], 1);
    let cancel = CancellationToken::new();
    let handle = spawn_calls(
        h.execution.clone(),
        h.ctx.clone(),
        vec![call("running", 0), call("waiting", 1)],
        h.agent_semaphore.clone(),
        cancel.clone(),
        h.catalog.clone(),
    );
    assert_eq!(h.started.recv().await.unwrap(), "running");

    cancel.cancel();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), h.started.recv())
            .await
            .is_err(),
        "cancelled Agent waiting for a permit must not start"
    );
    notify(&h.gates, "running");
    let results = handle.await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].provider_id, "provider-running");
}

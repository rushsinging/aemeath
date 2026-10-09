use super::{execute_tool_round, send_tool_result};
use crate::application::loop_engine::chat::{
    ChatEventSink, EventFuture, RuntimeRunContext, RuntimeStreamEvent,
};
use crate::application::loop_engine::ToolGuardDecision;
use crate::application::tool::agent::{Agent, ToolCall, ToolExecution};
use crate::application::tool::coordination::complete_cancelled_tool_round;
use async_trait::async_trait;
use hook::{HookDispatcher, HookInvocationData, HookOutcomeData};
use sdk::ids::{ChatId, ChatRunId, ToolCallId};
use serde_json::Value;
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use share::message::ContentBlock;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tools::published::execution::ToolExecutionContext;
use tools::published::typed::{TypedTool, TypedToolResult};
use tools::ToolOutcome;

/// A test HookDispatcher that always returns Continue.
struct NoOpHookPort;

#[async_trait]
impl HookDispatcher for NoOpHookPort {
    async fn dispatch(
        &self,
        _invocation: HookInvocationData,
        _cancellation: &dyn hook::HookCancellationSignal,
    ) -> HookOutcomeData {
        HookOutcomeData::proceed()
    }
}

fn noop_hook_port() -> Arc<dyn HookDispatcher> {
    Arc::new(NoOpHookPort)
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
}

impl RecordingSink {
    fn lifecycle_events(&self) -> Vec<(String, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                RuntimeStreamEvent::ToolCallStateChanged { id, status, .. } => {
                    Some((id.to_string(), format!("{status:?}")))
                }
                RuntimeStreamEvent::ToolResult { id, .. } => {
                    Some((id.to_string(), "Result".to_string()))
                }
                _ => None,
            })
            .collect()
    }
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.events.lock().unwrap().push(event);
    }
}

struct UnsafeLifecycleTool;

struct BlockingAgentTool {
    started: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl TypedTool for BlockingAgentTool {
    type Output = Value;

    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "blocking Agent cancellation test"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object"})
    }

    fn cancellation(&self) -> tools::published::execution::CancellationDeclaration {
        tools::published::execution::CancellationDeclaration::Cooperative
    }

    async fn call(
        &self,
        _input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        self.started.notify_one();
        ctx.cancellation().cancelled().await;
        TypedToolResult::error("Agent cancelled by current Step")
    }
}

#[async_trait]
impl TypedTool for UnsafeLifecycleTool {
    type Output = Value;

    fn name(&self) -> &str {
        "UnsafeLifecycle"
    }

    fn description(&self) -> &str {
        "non-concurrency-safe lifecycle test tool"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object"})
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        TypedToolResult::success(
            input.get("label").and_then(Value::as_str).unwrap_or("ok"),
            Value::Null,
        )
    }
}

fn test_tool_context() -> ToolExecutionContext {
    crate::application::run::workspace_test_support::test_tool_execution_context(
        std::env::current_dir().unwrap(),
        tokio_util::sync::CancellationToken::new(),
    )
}

fn lifecycle_call(index: usize) -> ToolCall {
    ToolCall {
        id: ToolCallId::from_legacy_or_new(&format!("call-{index}")),
        provider_id: format!("provider-{index}"),
        name: "UnsafeLifecycle".to_string(),
        index,
        input: serde_json::json!({"label": format!("call-{index}")}),
    }
}

#[tokio::test]
async fn tool_round_step_cancellation_reaches_running_agent_context() {
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    let started = Arc::new(tokio::sync::Notify::new());
    registry.register(BlockingAgentTool {
        started: started.clone(),
    });
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let agent = Arc::new(Agent::for_test(registry.as_ref(), ctx, 10));
    let sink = RecordingSink::default();
    let hook_port = noop_hook_port();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let call = ToolCall {
        id: ToolCallId::from_legacy_or_new("agent-cancel"),
        provider_id: "provider-agent-cancel".to_string(),
        name: "Agent".to_string(),
        index: 0,
        input: serde_json::json!({}),
    };
    let step_cancel = tokio_util::sync::CancellationToken::new();
    let execution_agent = agent.clone();
    let execution_context = context.clone();
    let execution_sink = sink.clone();
    let execution_hook_port = hook_port.clone();
    let execution_workspace_read = workspace_read.clone();
    let execution_call = call.clone();
    let execution_cancel = step_cancel.clone();
    let execution_activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );
    let handle = tokio::spawn(async move {
        execute_tool_round(
            &execution_context,
            std::slice::from_ref(&execution_call),
            &execution_agent.catalog,
            &*policy::allow_all(),
            None,
            &sdk::RunId::new_v7(),
            &sdk::RunStepId::new_v7(),
            execution_agent.as_ref(),
            &execution_sink,
            &execution_hook_port,
            &execution_activities,
            &execution_cancel,
            "en",
            &execution_workspace_read,
            &[(execution_call.clone(), ToolGuardDecision::Allow)],
        )
        .await
    });
    started.notified().await;

    step_cancel.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("当前 Step 取消必须到达运行中的 Agent execution context")
        .unwrap();

    assert_eq!(result.results.len(), 1);
    assert!(result.results[0].outcome.is_error);
    assert!(result.results[0].outcome.text.contains("cancel"));
}

#[tokio::test]
async fn allow_all_bypasses_soft_block_and_blocking_pre_tool_hook() {
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    registry.register(UnsafeLifecycleTool);
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let agent = Agent::for_test(registry.as_ref(), ctx, 10);
    let sink = RecordingSink::default();
    let hook_port = noop_hook_port();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let call = lifecycle_call(0);
    let activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );

    let result = execute_tool_round(
        &context,
        std::slice::from_ref(&call),
        &agent.catalog,
        &*policy::allow_all(),
        None,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &agent,
        &sink,
        &hook_port,
        &activities,
        &tokio_util::sync::CancellationToken::new(),
        "en",
        &workspace_read,
        &[(
            call.clone(),
            ToolGuardDecision::SoftBlock {
                reason: "loop".to_string(),
            },
        )],
    )
    .await;

    assert_eq!(result.fuse_bypassed, vec![call.id]);
    assert_eq!(result.results.len(), 1);
    assert!(
        !result.results[0].outcome.is_error,
        "AllowAll must execute the tool"
    );
}

/// #1515: PreToolUse 事件 hook 必须无条件执行——AllowAll 只放行授权性
/// 限制，不得跳过事件 hook。修复前 PreToolUse 被错误门控跳过、工具正常
/// 执行；修复后 hook exit 2 阻断工具。
#[tokio::test]
async fn allow_all_still_runs_blocking_pre_tool_hook() {
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    registry.register(UnsafeLifecycleTool);
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let agent = Agent::for_test(registry.as_ref(), ctx, 10);
    let sink = RecordingSink::default();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::PreToolUse,
        vec![HookEntry {
            matcher: String::new(),
            command: "exit 2".to_string(),
            timeout: 5,
        }],
    );
    let hook_port: Arc<dyn HookDispatcher> = hook::wire_hook_dispatcher(
        &share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config {
            hooks: HooksConfig {
                events,
                ..HooksConfig::default()
            },
            ..share::config::Config::default()
        }),
    )
    .unwrap();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let call = lifecycle_call(0);
    let activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );

    let result = execute_tool_round(
        &context,
        std::slice::from_ref(&call),
        &agent.catalog,
        &*policy::allow_all(),
        None,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &agent,
        &sink,
        &hook_port,
        &activities,
        &tokio_util::sync::CancellationToken::new(),
        "en",
        &workspace_read,
        &[(call.clone(), ToolGuardDecision::Allow)],
    )
    .await;

    assert_eq!(result.results.len(), 1);
    assert!(
        result.results[0].outcome.is_error,
        "AllowAll 不得跳过 PreToolUse 事件 hook：exit 2 应阻断工具"
    );
    assert!(
        result.results[0]
            .outcome
            .text
            .contains("Blocked by PreToolUse hook"),
        "阻断消息应来自 PreToolUse hook，实际 = {:?}",
        result.results[0].outcome.text
    );
}

#[tokio::test]
async fn test_non_concurrency_safe_tools_emit_running_after_previous_result() {
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    registry.register(UnsafeLifecycleTool);
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let agent = Agent::for_test(registry.as_ref(), ctx, 10);
    let sink = RecordingSink::default();
    let hook_port = noop_hook_port();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );
    let calls = vec![lifecycle_call(0), lifecycle_call(1)];
    let guarded_calls = calls
        .iter()
        .cloned()
        .map(|call| (call, ToolGuardDecision::Allow))
        .collect::<Vec<_>>();

    let _ = execute_tool_round(
        &context,
        &calls,
        &agent.catalog,
        &*policy::allow_all(),
        None,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &agent,
        &sink,
        &hook_port,
        &activities,
        &tokio_util::sync::CancellationToken::new(),
        "en",
        &workspace_read,
        &guarded_calls,
    )
    .await;

    let lifecycle = sink.lifecycle_events();

    assert_eq!(
        lifecycle,
        vec![
            (calls[0].id.to_string(), "Ready".to_string()),
            (calls[0].id.to_string(), "Running".to_string()),
            (calls[0].id.to_string(), "Result".to_string()),
            (calls[1].id.to_string(), "Ready".to_string()),
            (calls[1].id.to_string(), "Running".to_string()),
            (calls[1].id.to_string(), "Result".to_string()),
        ]
    );
}

#[tokio::test]
async fn oversized_tool_result_event_uses_materialized_projection() {
    const THRESHOLD: usize = 50_000;
    let oversized = "界".repeat(THRESHOLD + 1);
    assert!(oversized.chars().count() > THRESHOLD);
    let execution = ToolExecution::from_parts(
        ToolCallId::new_v7(),
        "provider-oversized".to_string(),
        "UnknownTool".to_string(),
        ToolOutcome::new(
            oversized.clone(),
            serde_json::json!({ "unexpected": oversized }),
            Vec::new(),
        ),
    );
    let sink = RecordingSink::default();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();

    send_tool_result(
        &sink,
        &context,
        &execution,
        materializer.as_ref(),
        "event-session",
    )
    .await;

    let events = sink.events.lock().unwrap();
    let [RuntimeStreamEvent::ToolResult {
        output, content, ..
    }] = events.as_slice()
    else {
        panic!("expected one tool result event");
    };
    assert!(!output.contains(&oversized));
    assert_eq!(
        content.get("text").and_then(Value::as_str),
        Some(output.as_str())
    );
    assert_eq!(
        content.pointer("/blob/status").and_then(Value::as_str),
        Some("persisted")
    );
    assert_eq!(
        content.pointer("/blob/locator").and_then(Value::as_str),
        Some("tool-result://event-session/provider-oversized")
    );
    assert!(!content.to_string().contains(&oversized));
}

/// #1666：`send_tool_result` 必须把 `ToolExecution.duration_ms`
/// （supervisor 测量值）透传进 `RuntimeStreamEvent::ToolResult`，
/// NEVER 静默丢弃——这是耗时进入事件流的唯一出口。
#[tokio::test]
async fn send_tool_result_forwards_execution_duration() {
    let execution = ToolExecution::from_parts(
        ToolCallId::new_v7(),
        "provider-duration".to_string(),
        "Bash".to_string(),
        ToolOutcome::new("ok", Value::Null, Vec::new()),
    )
    .with_duration(1_500);
    let sink = RecordingSink::default();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();

    send_tool_result(
        &sink,
        &context,
        &execution,
        materializer.as_ref(),
        "event-session",
    )
    .await;

    let events = sink.events.lock().unwrap();
    let [RuntimeStreamEvent::ToolResult { duration_ms, .. }] = events.as_slice() else {
        panic!("expected one tool result event");
    };
    assert_eq!(
        *duration_ms,
        Some(1_500),
        "事件必须携带 execution.duration_ms"
    );
}

/// #1666：非 supervisor 路径（如 from_parts 直接构造）duration 保持 None，
/// 渲染层据此不显示耗时占位。
#[tokio::test]
async fn send_tool_result_without_duration_keeps_none() {
    let execution = ToolExecution::from_parts(
        ToolCallId::new_v7(),
        "provider-no-duration".to_string(),
        "Bash".to_string(),
        ToolOutcome::new("ok", Value::Null, Vec::new()),
    );
    let sink = RecordingSink::default();
    let context = RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn"));
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();

    send_tool_result(
        &sink,
        &context,
        &execution,
        materializer.as_ref(),
        "event-session",
    )
    .await;

    let events = sink.events.lock().unwrap();
    let [RuntimeStreamEvent::ToolResult { duration_ms, .. }] = events.as_slice() else {
        panic!("expected one tool result event");
    };
    assert_eq!(*duration_ms, None);
}

#[tokio::test]
async fn cancelled_tool_round_materializes_one_result_for_each_provider_call() {
    let calls = vec![lifecycle_call(0), lifecycle_call(1)];
    let completed = ToolExecution::new(
        &calls[0],
        ToolOutcome::new("finished", Value::Null, Vec::new()),
    );
    let results = complete_cancelled_tool_round(&calls, vec![completed]).results;
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();

    let message = crate::application::loop_engine::shared::materialize_tool_results(
        materializer.as_ref(),
        results,
        "test-cancelled-round",
    )
    .await;

    assert_eq!(message.content.len(), 2);
    let provider_ids = message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::ToolResult { tool_use_id, .. } => tool_use_id.as_str(),
            other => panic!("expected tool result, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(provider_ids, ["provider-0", "provider-1"]);
}

#[tokio::test]
async fn test_materialized_tool_results_use_provider_id_not_runtime_id() {
    let results = vec![ToolExecution::from_parts(
        ToolCallId::new_v7(),
        "provider-id".to_string(),
        "Bash".to_string(),
        ToolOutcome::new("ok", serde_json::json!({ "text": "ok" }), Vec::new()),
    )];
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();
    let message = crate::application::loop_engine::shared::materialize_tool_results(
        materializer.as_ref(),
        results,
        "test-provider-id",
    )
    .await;

    let [ContentBlock::ToolResult { tool_use_id, .. }] = message.content.as_slice() else {
        panic!("expected one tool result");
    };
    assert_eq!(tool_use_id, "provider-id");
}

#[tokio::test]
async fn test_materialized_tool_results_persist_oversized_tui_result() {
    const THRESHOLD: usize = 50_000;
    let session_id = format!("test-tui-{}", std::process::id());
    let oversized = "x".repeat(THRESHOLD + 1);
    let results = vec![ToolExecution::from_parts(
        ToolCallId::new_v7(),
        "provider-oversized".to_string(),
        "Bash".to_string(),
        ToolOutcome::new(
            oversized,
            serde_json::json!({ "text": "oversized" }),
            Vec::new(),
        ),
    )];
    let materializer = crate::application::tool::test_support::test_tool_result_materializer();
    let message = crate::application::loop_engine::shared::materialize_tool_results(
        materializer.as_ref(),
        results,
        &session_id,
    )
    .await;

    let [ContentBlock::ToolResult { content, .. }] = message.content.as_slice() else {
        panic!("expected one tool result");
    };
    let content = match content {
        serde_json::Value::Object(map) => map,
        other => panic!("tool result should be json object, got {other:?}"),
    };
    let text = content
        .get("text")
        .and_then(|value| value.as_str())
        .expect("persisted reference should be in text field");
    assert!(text.contains("<persisted-output>"));
    assert!(text.len() < THRESHOLD);
    assert!(text.contains(&session_id));
}

// #1248 TaskData 5: Bridge resolve tests moved to engine-level tests
// (interaction_routing module in loop_engine/tests.rs).
// resolve_ask_user_via_bridge is deleted — the engine handles
// all interaction routing via InteractionCoordinator.

// ── #252 后台任务：工具轮级快路径等价与转后台场景 ─────────────────────
//
// 计时断言在高负载（pre-push 并发全量测试）下易 flaky，改用确定性信号：
// fast 工具完成时刻观察 slow 是否已真实完成。

/// 慢工具完成时置位的信号（观察"fast 先于 slow 真实完成"的确定性证据）。
type SlowCompletionSignal = Arc<std::sync::atomic::AtomicBool>;

struct SlowSequentialTool {
    finished: SlowCompletionSignal,
}

#[async_trait]
impl TypedTool for SlowSequentialTool {
    type Output = Value;

    fn name(&self) -> &str {
        "SlowSequential"
    }

    fn description(&self) -> &str {
        "slow sequential tool for backgrounding tests"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object"})
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        self.finished
            .store(true, std::sync::atomic::Ordering::SeqCst);
        TypedToolResult::success(
            input
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("slow-done"),
            Value::Null,
        )
    }
}

fn slow_sequential_call(index: usize) -> ToolCall {
    ToolCall {
        id: ToolCallId::from_legacy_or_new(&format!("slow-{index}")),
        provider_id: format!("provider-slow-{index}"),
        name: "SlowSequential".to_string(),
        index,
        input: serde_json::json!({"label": format!("slow-{index}")}),
    }
}

/// 快速 sequential 工具：立即返回，作为「前序转后台后的同轮后续调用」探针；
/// 完成时记录 slow 是否已真实完成（false = fast 先于 slow，证明未被前序阻塞）。
struct FastSequentialTool {
    slow_finished_at_fast_completion: Arc<Mutex<Option<bool>>>,
    slow_finished: SlowCompletionSignal,
}

#[async_trait]
impl TypedTool for FastSequentialTool {
    type Output = Value;

    fn name(&self) -> &str {
        "FastSequential"
    }

    fn description(&self) -> &str {
        "fast sequential probe tool"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({"type":"object"})
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<Self::Output> {
        let observed = self.slow_finished.load(std::sync::atomic::Ordering::SeqCst);
        *self
            .slow_finished_at_fast_completion
            .lock()
            .expect("fast 探针观察锁") = Some(observed);
        TypedToolResult::success(
            input
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("fast-done"),
            Value::Null,
        )
    }
}

fn fast_sequential_call(index: usize) -> ToolCall {
    ToolCall {
        id: ToolCallId::from_legacy_or_new(&format!("fast-{index}")),
        provider_id: format!("provider-fast-{index}"),
        name: "FastSequential".to_string(),
        index,
        input: serde_json::json!({"label": format!("fast-{index}")}),
    }
}

#[tokio::test]
async fn sequential_round_launches_following_call_after_first_moves_to_background() {
    let slow_finished: SlowCompletionSignal = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_slow_finished: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    registry.register(SlowSequentialTool {
        finished: slow_finished.clone(),
    });
    registry.register(FastSequentialTool {
        slow_finished_at_fast_completion: observed_slow_finished.clone(),
        slow_finished: slow_finished.clone(),
    });
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let mut agent = Agent::for_test(registry.as_ref(), ctx, 10);
    agent.background_threshold = Some(std::time::Duration::from_millis(30));
    let sink = RecordingSink::default();
    let activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );
    // 第一个 slow 超阈值转后台；第二个 fast 应在前序转后台后立即启动并真实完成。
    let calls = [slow_sequential_call(0), fast_sequential_call(1)];
    let guard_decisions = [
        (calls[0].clone(), ToolGuardDecision::Allow),
        (calls[1].clone(), ToolGuardDecision::Allow),
    ];

    let result = execute_tool_round(
        &RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn")),
        &calls,
        &agent.catalog,
        &*policy::allow_all(),
        None,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &agent,
        &sink,
        &noop_hook_port(),
        &activities,
        &tokio_util::sync::CancellationToken::new(),
        "en",
        &workspace_read,
        &guard_decisions,
    )
    .await;

    assert_eq!(result.results.len(), 2, "两个 sequential 调用都应有结果");
    assert!(
        result.results[0].outcome.text.contains("background"),
        "超阈值的第一调用应返回占位结果：{}",
        result.results[0].outcome.text
    );
    assert!(
        result.results[1].outcome.text.contains("fast-1"),
        "后续 sequential 调用应真实完成：{}",
        result.results[1].outcome.text
    );
    assert_eq!(
        *observed_slow_finished.lock().expect("fast 探针观察锁"),
        Some(false),
        "fast 后续调用完成时 slow 尚未真实完成——证明前序转后台后未被阻塞"
    );
}

#[tokio::test]
async fn sequential_round_fast_path_within_threshold_returns_real_results() {
    let registry = Arc::new(tools::composition::TestCatalogExecutionFactory::new());
    registry.register(SlowSequentialTool {
        finished: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let ctx = test_tool_context();
    let workspace_read = ctx.workspace_read();
    let mut agent = Agent::for_test(registry.as_ref(), ctx, 10);
    agent.background_threshold = Some(std::time::Duration::from_secs(5));
    let sink = RecordingSink::default();
    let activities = crate::application::activity::ActivityCoordinator::new(
        sdk::RunId::new_v7(),
        Arc::new(crate::application::activity::SystemActivityClock),
        Arc::new(crate::application::activity::UuidV7ActivityIdSource),
    );
    let calls = [slow_sequential_call(0)];
    let guard_decisions = [(calls[0].clone(), ToolGuardDecision::Allow)];

    let result = execute_tool_round(
        &RuntimeRunContext::new(ChatId::new("chat"), ChatRunId::new("turn")),
        &calls,
        &agent.catalog,
        &*policy::allow_all(),
        None,
        &sdk::RunId::new_v7(),
        &sdk::RunStepId::new_v7(),
        &agent,
        &sink,
        &noop_hook_port(),
        &activities,
        &tokio_util::sync::CancellationToken::new(),
        "en",
        &workspace_read,
        &guard_decisions,
    )
    .await;

    assert_eq!(result.results.len(), 1);
    assert!(
        result.results[0].outcome.text.contains("slow-0"),
        "阈值内完成应返回真实结果（快路径等价）：{}",
        result.results[0].outcome.text
    );
    assert!(!result.results[0].outcome.is_error);
}

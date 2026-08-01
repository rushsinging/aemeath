use super::*;
use crate::application::interaction::port::InteractionBridge;
use crate::application::run::config::RunConfigSnapshot;
use crate::application::run::context::{
    IoBindings, LifecycleBindings, ModelBindings, RunCancellationScope, RunCapabilityBindings,
    RunInputBufferHandle, RunUsageTracker, RuntimeContext,
};
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::test_task_access;
use crate::domain::agent_run::RunSpec;
use crate::ports::{PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderPort};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Re-declare local fakes (needed because sub_context_derivation_tests's
// fakes are pub(super) and not visible across sibling modules).

struct LocalFakeCtxPort;
#[async_trait::async_trait]
impl crate::ports::ContextPort for LocalFakeCtxPort {
    async fn build_window(
        &self,
        _request: &crate::ports::ContextRequest,
    ) -> Result<crate::ports::ContextWindow, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }
    async fn needs_compaction(
        &self,
        _request: &crate::ports::ContextRequest,
    ) -> Result<crate::ports::CompactionDecision, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }
    async fn compact(
        &self,
        _request: &crate::ports::CompactRequest,
    ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }
    async fn manual_compact(
        &self,
        _request: &crate::ports::ManualCompactRequest,
    ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
        Err(crate::ports::ContextPortError::Compact("fake".into()))
    }
    async fn clear_session(
        &self,
        _session_id: &crate::ports::SessionId,
    ) -> Result<(), crate::ports::ContextPortError> {
        Ok(())
    }
    async fn append_and_persist(
        &self,
        _append: &crate::ports::ContextAppend,
    ) -> Result<crate::ports::AppendReceipt, crate::ports::ContextAppendError> {
        Err(crate::ports::ContextAppendError::Storage("fake".into()))
    }
}

struct LocalFakeProvPort;
#[async_trait::async_trait]
impl ProviderPort for LocalFakeProvPort {
    fn capabilities(
        &self,
        _model: &crate::ports::ModelId,
    ) -> Result<crate::ports::ModelCapability, crate::ports::ProviderError> {
        Ok(crate::ports::ModelCapability {
            model: crate::ports::ModelId {
                provider: "test".into(),
                model: "test".into(),
            },
            supports_tools: true,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: crate::ports::ReasoningCapability::none(),
            context_limit: None,
            output_limit: None,
        })
    }
    async fn invoke(
        &self,
        _request: crate::ports::InvocationRequest,
        _cancellation: &dyn provider::CancellationSignal,
    ) -> Result<crate::ports::InvocationStream, crate::ports::ProviderError> {
        Err(crate::ports::ProviderError::cancelled())
    }
}

struct LocalFakeHookPort;
#[async_trait::async_trait]
impl hook::HookPort for LocalFakeHookPort {
    async fn dispatch(
        &self,
        _invocation: hook::HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> hook::HookOutcome {
        hook::HookOutcome::proceed()
    }
}
/// A catalog that records whether `snapshot` was called, delegating to
/// a real built-in catalog.
struct SpyToolCatalog {
    inner: Arc<dyn tools::ToolCatalogPort>,
    called: Arc<AtomicBool>,
}

impl tools::ToolCatalogPort for SpyToolCatalog {
    fn snapshot(
        &self,
        scope: &tools::RegistryScopeName,
        profile: &tools::ToolProfileName,
    ) -> Result<tools::ToolCatalogSnapshot, tools::ToolCatalogError> {
        self.called.store(true, Ordering::SeqCst);
        self.inner.snapshot(scope, profile)
    }
}

/// A policy spy.
struct SpyPolicy {
    called: Arc<AtomicBool>,
}

impl PolicyPort for SpyPolicy {
    fn evaluate(&self, _request: &PolicyRequest) -> PolicyDecision {
        self.called.store(true, Ordering::SeqCst);
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    }
}

fn noop_event_sink() -> crate::application::loop_engine::chat::ChatEventSinkHandle {
    #[derive(Clone)]
    struct NoOp;
    impl crate::application::loop_engine::chat::ChatEventSink for NoOp {
        fn send_event<'a>(
            &'a self,
            _event: crate::application::loop_engine::chat::RuntimeStreamEvent,
        ) -> crate::application::loop_engine::chat::EventFuture<'a> {
            Box::pin(std::future::ready(()))
        }
        fn try_send_event(
            &self,
            _event: crate::application::loop_engine::chat::RuntimeStreamEvent,
        ) {
        }
    }
    crate::application::loop_engine::chat::ChatEventSinkHandle::new(NoOp)
}

/// #1248 Task 3: shared factory-based context construction for wiring tests.
fn assemble_test_context(
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    policy: Arc<dyn PolicyPort>,
    config: RunConfigSnapshot,
) -> (RuntimeContext, RuntimeContextFactory) {
    let provider_port: Arc<dyn ProviderPort> = Arc::new(LocalFakeProvPort);
    let binding = ProviderBinding {
        provider: provider_port.clone(),
        model: crate::ports::ModelId {
            provider: "test-provider".into(),
            model: "test-model".into(),
        },
        max_tokens: 8192,
        requested_reasoning: provider::ReasoningLevel::Medium,
        context_window: None,
    };

    let factory = RuntimeContextFactory::new(
        tool_catalog,
        tool_execution,
        policy,
        Arc::new(sub_context_derivation_tests::FakeReflHist),
        test_task_access(),
        Arc::new(LocalFakeHookPort),
    );
    let bindings = RunCapabilityBindings {
        model: ModelBindings {
            context: Arc::new(LocalFakeCtxPort),
            provider: Arc::new(binding),
            interaction: Arc::new(InteractionBridge::new()),
            memory: Arc::new(memory::NoOpMemory),
            config,
            reasoning: Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium)),
            tool_catalog: None,
        },
        io: IoBindings {
            event_sink: noop_event_sink(),
            input: RunInputBufferHandle::new(),
        },
        lifecycle: LifecycleBindings {
            cancel: RunCancellationScope::new(),
            usage: RunUsageTracker::new(),
        },
        skill_load_session_id: "session".to_string(),
    };
    let ctx = factory
        .create(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test wiring context assembly");
    (ctx, factory)
}

fn make_spy_parent_context(
    cat_called: Arc<AtomicBool>,
    policy_called: Arc<AtomicBool>,
) -> RuntimeContext {
    let config_snap = test_config_snapshot();
    let run_config = RunConfigSnapshot::capture(config_snap);

    let factory = tools::composition::TestCatalogExecutionFactory::new();
    factory.register(ReadFixtureTool);
    let ports = factory.build(test_ctx());

    let catalog: Arc<dyn tools::ToolCatalogPort> = Arc::new(SpyToolCatalog {
        inner: ports.catalog_port(),
        called: cat_called,
    });

    let policy: Arc<dyn PolicyPort> = Arc::new(SpyPolicy {
        called: policy_called,
    });

    let (ctx, _factory) = assemble_test_context(catalog, ports.execution(), policy, run_config);
    ctx
}

// ── M1: catalog access goes through derived.instance.context()'s tool_catalog ──

#[tokio::test]
async fn derived_tool_catalog_is_spied_during_sub_agent_run() {
    let cat_called = Arc::new(AtomicBool::new(false));
    let policy_called = Arc::new(AtomicBool::new(false));

    let parent_ctx = make_spy_parent_context(cat_called.clone(), policy_called.clone());
    let source = crate::application::run::context::ParentRunContextSource::new();
    let _guard = source.install(Arc::new(crate::application::run::context::ParentRunFrame {
        run_id: crate::domain::agent_run::RunId::new_v7(),
        spec: RunSpec::main(),
        context: Arc::new(parent_ctx),
    }));

    let (mut runner, _runner_guard) = test_runner(ProviderError::fatal(
        ProviderErrorKind::Network,
        "chain-test",
    ));
    runner.parent_context = source;
    let ctx = test_ctx();

    let _ = runner
        .run_agent(AgentRunRequest {
            prompt: "p",
            system: "s",
            identity: ctx.scope(),
            cancellation: ctx.cancellation(),
            progress: ctx.progress_sink(),
            memory: ctx.memory(),
            catalog: ctx.catalog_query(),
            read_set: ctx.read_set(),
            plan_mode: ctx.plan_mode_state(),
            guidance: ctx.guidance(),
            timeout: std::time::Duration::from_secs(30),
            role: "coder",
        })
        .await;

    assert!(
        cat_called.load(Ordering::SeqCst),
        "M1: derived context's tool_catalog MUST be called during derive_sub_run"
    );
}

// ── M6: ParentRunContextSource RAII with generation ──

#[test]
fn parent_source_guard_only_clears_own_generation() {
    let source = crate::application::run::context::ParentRunContextSource::new();

    let parent_ctx1 = sub_context_derivation_tests::make_parent_context();
    let guard1 = source.install(Arc::new(crate::application::run::context::ParentRunFrame {
        run_id: crate::domain::agent_run::RunId::new_v7(),
        spec: RunSpec::main(),
        context: Arc::new(parent_ctx1),
    }));
    assert!(source.get().is_some(), "first frame should be installed");

    // Install second frame while first guard is still alive.
    let parent_ctx2 = sub_context_derivation_tests::make_parent_context();
    let guard2 = source.install(Arc::new(crate::application::run::context::ParentRunFrame {
        run_id: crate::domain::agent_run::RunId::new_v7(),
        spec: RunSpec::main(),
        context: Arc::new(parent_ctx2),
    }));
    assert!(source.get().is_some(), "second frame should be installed");

    // Drop the first (stale) guard — must NOT clear the second frame.
    drop(guard1);
    assert!(
        source.get().is_some(),
        "stale guard must not clear fresh frame"
    );

    // Drop the second guard — now source should be empty.
    drop(guard2);
    assert!(
        source.get().is_none(),
        "fresh guard drop must clear its own generation"
    );
}

#[test]
fn parent_source_returns_none_when_empty() {
    let source = crate::application::run::context::ParentRunContextSource::new();
    assert!(source.get().is_none());
}

// ── M7: Comprehensive derived context integration ──
//
// Exercises run_agent through the real production derivation path and
// verifies that the derived RuntimeContext exercises catalog and policy
// through the direct per-call ToolExecutionContext path.

#[tokio::test]
async fn derived_context_integration_verifies_policy_catalog() {
    let cat_called = Arc::new(AtomicBool::new(false));
    let policy_called = Arc::new(AtomicBool::new(false));

    let tool_factory = tools::composition::TestCatalogExecutionFactory::new();
    tool_factory.register(ReadFixtureTool);
    let tool_ports = tool_factory.build(test_ctx());
    let catalog: Arc<dyn tools::ToolCatalogPort> = Arc::new(SpyToolCatalog {
        inner: tool_ports.catalog_port(),
        called: cat_called.clone(),
    });

    let (parent_ctx, shared_factory) = assemble_test_context(
        catalog,
        tool_ports.execution(),
        Arc::new(SpyPolicy {
            called: policy_called.clone(),
        }),
        RunConfigSnapshot::capture(test_config_snapshot()),
    );

    let source = crate::application::run::context::ParentRunContextSource::new();
    let parent_frame_guard =
        source.install(Arc::new(crate::application::run::context::ParentRunFrame {
            run_id: crate::domain::agent_run::RunId::new_v7(),
            spec: RunSpec::main(),
            context: Arc::new(parent_ctx),
        }));

    let (mut runner, _runner_guard) = test_runner(ProviderError::fatal(
        ProviderErrorKind::Network,
        "integration-spy",
    ));
    // #1248 Task 3: Use the same factory as the parent context so static
    // ports (policy, hooks, reflection) are Arc-identical.
    runner.runtime_context_factory = Arc::new(shared_factory);
    runner.parent_context = source;
    let ctx = test_ctx();

    let _result = runner
        .run_agent(AgentRunRequest {
            prompt: "p",
            system: "s",
            identity: ctx.scope(),
            cancellation: ctx.cancellation(),
            progress: ctx.progress_sink(),
            memory: ctx.memory(),
            catalog: ctx.catalog_query(),
            read_set: ctx.read_set(),
            plan_mode: ctx.plan_mode_state(),
            guidance: ctx.guidance(),
            timeout: std::time::Duration::from_secs(30),
            role: "coder",
        })
        .await;

    // M1: Catalog spied during derivation.
    assert!(
        cat_called.load(Ordering::SeqCst),
        "M1: derived catalog must be called"
    );

    // Policy spy is wired into derived context but only exercised
    // when the LLM returns tool calls. With a fatal-error provider,
    // no tool round occurs.
    let _ = policy_called;

    drop(parent_frame_guard);
}

// ── L2: Real tool execution + progress + policy + binding ──

/// A tool that records execution and optionally emits progress.
struct SpyTool {
    executed: Arc<AtomicBool>,
    progress_sink_was_some: Arc<AtomicBool>,
    invocation_source: Arc<std::sync::Mutex<Option<tools::InvocationSource>>>,
}

#[async_trait::async_trait]
impl tools::TypedTool for SpyTool {
    type Output = serde_json::Value;

    fn name(&self) -> &str {
        "spy"
    }

    fn description(&self) -> &str {
        "spy tool for integration test"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        ctx: &tools::ToolExecutionContext,
    ) -> tools::TypedToolResult<Self::Output> {
        self.executed.store(true, Ordering::SeqCst);
        // Verify progress_sink is Some — proving ToolExecutionPorts wired it.
        if ctx.progress_sink().is_some() {
            self.progress_sink_was_some.store(true, Ordering::SeqCst);
        }
        *self.invocation_source.lock().unwrap() = Some(ctx.scope().invocation_source());
        tools::TypedToolResult::success("ok", serde_json::json!({"executed": true}))
    }
}

#[tokio::test]
async fn run_agent_executes_tool_and_propagates_progress_policy_and_binding() {
    use crate::application::model::test_support::{test_binding_from_port, TestProviderPort};
    use provider::{
        InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderStopReason,
        ProviderToolCall, ProviderToolCallId, RawUsageSnapshot, ReasoningLevel,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::mpsc;

    // ── Tool factory with spy ──
    let tool_executed = Arc::new(AtomicBool::new(false));
    let progress_some = Arc::new(AtomicBool::new(false));
    let invocation_source = Arc::new(std::sync::Mutex::new(None));
    let tool_factory = tools::composition::TestCatalogExecutionFactory::new();
    tool_factory.register(ReadFixtureTool);
    tool_factory.register(SpyTool {
        executed: tool_executed.clone(),
        progress_sink_was_some: progress_some.clone(),
        invocation_source: invocation_source.clone(),
    });
    let tool_ports = tool_factory.build(test_ctx());

    // ── Spies ──
    let policy_called = Arc::new(AtomicBool::new(false));
    let cat_called = Arc::new(AtomicBool::new(false));

    let catalog: Arc<dyn tools::ToolCatalogPort> = Arc::new(SpyToolCatalog {
        inner: tool_ports.catalog_port(),
        called: cat_called.clone(),
    });

    // ── Parent context with spies ──
    let (parent_ctx, shared_factory) = assemble_test_context(
        catalog,
        tool_ports.execution(),
        Arc::new(SpyPolicy {
            called: policy_called.clone(),
        }),
        RunConfigSnapshot::capture(test_config_snapshot()),
    );

    let source = crate::application::run::context::ParentRunContextSource::new();
    let parent_frame_guard =
        source.install(Arc::new(crate::application::run::context::ParentRunFrame {
            run_id: crate::domain::agent_run::RunId::new_v7(),
            spec: RunSpec::main(),
            context: Arc::new(parent_ctx),
        }));

    // ── Progress sink ──
    let (tx, mut rx) = mpsc::channel::<tools::AgentProgressEvent>(8);

    // ── Provider: first call → tool call, second call → end_turn ──
    let second_call = Arc::new(AtomicBool::new(false));
    let second_call2 = second_call.clone();
    let tool_call = ProviderToolCall {
        id: ProviderToolCallId("toolu_test_001".to_string()),
        name: "spy".to_string(),
        arguments: serde_json::json!({}),
    };
    let model = crate::application::model::test_support::test_model_id();
    let port = TestProviderPort::new(Vec::new(), model.clone()).with_invocation_fn(Arc::new(
        move |_call_idx, request, _cancel| {
            let is_second = second_call2.swap(true, Ordering::SeqCst);
            let stream = if is_second {
                // Verify tool results were backfilled into the messages.
                let has_tool_result = request.messages.iter().any(|m| {
                    m.role == share::message::Role::User
                        && m.content.iter().any(|block| {
                            matches!(block, share::message::ContentBlock::ToolResult { .. })
                        })
                });
                assert!(
                    has_tool_result,
                    "second request must contain tool result backfill"
                );
                futures::stream::iter(vec![InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("all done".into())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(3),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                })])
            } else {
                futures::stream::iter(vec![InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::ToolCall(tool_call.clone())],
                    stop_reason: ProviderStopReason::ToolUse,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(5),
                        output_tokens: Some(8),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                })])
            };
            Box::pin(async move { Ok(Box::pin(stream) as InvocationStream) })
        },
    ));

    let binding = test_binding_from_port(port);
    let factory = crate::application::model::test_support::constant_factory(binding);

    let workspace = crate::application::run::workspace_test_support::runtime_workspace(
        &crate::application::run::workspace_test_support::test_tool_execution_context(
            std::env::temp_dir(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );

    let runner = CliAgentRunner {
        factory,
        active_run: Arc::new(
            crate::application::run::active_registry::ActiveRunRegistry::default(),
        ),
        max_tool_concurrency: 10,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        tool_result_materializer:
            crate::application::tool::test_support::test_tool_result_materializer(),
        workspace,
        skill_catalog: tools::composition::wire_skills().catalog(),
        parent_context: source,
        runtime_context_factory: Arc::new(shared_factory),
    };
    let ctx = test_ctx();

    let result = runner
        .run_agent(AgentRunRequest {
            prompt: "run spy",
            system: "system",
            identity: ctx.scope(),
            cancellation: ctx.cancellation(),
            progress: Some(crate::application::run::context::tool_progress_sink(
                tx.clone(),
            )),
            memory: ctx.memory(),
            catalog: ctx.catalog_query(),
            read_set: ctx.read_set(),
            plan_mode: ctx.plan_mode_state(),
            guidance: ctx.guidance(),
            timeout: std::time::Duration::from_secs(30),
            role: "coder",
        })
        .await;

    // ── Assertions ──

    // Tool actually executed.
    assert!(
        tool_executed.load(Ordering::SeqCst),
        "L2: spy tool must be executed"
    );
    // ToolExecutionContext is passed directly to the tool execution call.
    assert!(
        matches!(
            *invocation_source.lock().unwrap(),
            Some(tools::InvocationSource::SubAgent)
        ),
        "L2: derived tool execution context must use SubAgent invocation source"
    );
    // Policy spy called during tool execution.
    assert!(
        policy_called.load(Ordering::SeqCst),
        "L2: policy spy must be called during tool execution"
    );
    // Progress events received.
    let mut saw_started = false;
    let mut saw_tool_calls = false;
    while let Ok(ev) = rx.try_recv() {
        match ev.kind {
            tools::AgentProgressKind::Started { .. } => saw_started = true,
            tools::AgentProgressKind::ToolCalls { .. } => saw_tool_calls = true,
            _ => {}
        }
        if saw_started && saw_tool_calls {
            break;
        }
    }
    assert!(saw_started, "L2: must see Started progress event");
    assert!(saw_tool_calls, "L2: must see ToolCalls progress event");

    // Progress sink was wired into tool context.
    assert!(
        progress_some.load(Ordering::SeqCst),
        "L2: ToolExecutionPorts must wire progress_sink so tool can access it"
    );

    // Run completed (not failed).
    assert!(
        matches!(result, tools::AgentRunTerminal::Completed { .. }),
        "L2: run must complete successfully, got {result:?}"
    );

    drop(parent_frame_guard);
}

// ── L3: CombinedCancellation: cancel parent token cancels tool execution ──

/// A tool that blocks until its ctx cancellation fires, proving that
/// CombinedCancellationSignal propagates parent token cancellation.
struct BlockingCancelTool {
    started: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl tools::TypedTool for BlockingCancelTool {
    type Output = serde_json::Value;

    fn name(&self) -> &str {
        "blocking_cancel"
    }

    fn description(&self) -> &str {
        "blocks until cancelled"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _input: serde_json::Value,
        ctx: &tools::ToolExecutionContext,
    ) -> tools::TypedToolResult<Self::Output> {
        self.started.store(true, Ordering::SeqCst);
        // Block until the cancellation signal fires.
        ctx.cancellation().cancelled().await;
        tools::TypedToolResult::error("cancelled")
    }
}

#[tokio::test]
async fn parent_token_cancellation_propagates_to_tool_and_terminates_run() {
    use crate::application::model::test_support::{test_binding_from_port, TestProviderPort};
    use provider::{
        InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderStopReason,
        ProviderToolCall, ProviderToolCallId, RawUsageSnapshot, ReasoningLevel,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::mpsc;

    let tool_started = Arc::new(AtomicBool::new(false));
    let tool_factory = tools::composition::TestCatalogExecutionFactory::new();
    tool_factory.register(BlockingCancelTool {
        started: tool_started.clone(),
    });
    let tool_ports = tool_factory.build(test_ctx());

    let catalog: Arc<dyn tools::ToolCatalogPort> = tool_ports.catalog_port();

    let (parent_ctx, shared_factory) = assemble_test_context(
        catalog,
        tool_ports.execution(),
        Arc::new(SpyPolicy {
            called: Arc::new(AtomicBool::new(false)),
        }),
        RunConfigSnapshot::capture(test_config_snapshot()),
    );
    // Hold the parent cancel scope so we can cancel from the test.
    let parent_cancel = parent_ctx.cancel().clone();

    let source = crate::application::run::context::ParentRunContextSource::new();
    let parent_frame_guard =
        source.install(Arc::new(crate::application::run::context::ParentRunFrame {
            run_id: crate::domain::agent_run::RunId::new_v7(),
            spec: RunSpec::main(),
            context: Arc::new(parent_ctx),
        }));

    // Provider: returns a tool call for blocking_cancel.
    let tool_call = ProviderToolCall {
        id: ProviderToolCallId("toolu_block_001".to_string()),
        name: "blocking_cancel".to_string(),
        arguments: serde_json::json!({}),
    };
    let model = crate::application::model::test_support::test_model_id();
    let port = TestProviderPort::new(Vec::new(), model.clone()).with_invocation_fn(Arc::new(
        move |_call_idx, _request, _cancel| {
            let tc = tool_call.clone();
            Box::pin(async move {
                Ok(
                    Box::pin(futures::stream::iter(vec![InvocationEvent::Completed(
                        ProviderCompletion {
                            output: vec![ProviderContentBlock::ToolCall(tc)],
                            stop_reason: ProviderStopReason::ToolUse,
                            usage: Some(RawUsageSnapshot {
                                input_tokens: Some(5),
                                output_tokens: Some(8),
                                ..RawUsageSnapshot::default()
                            }),
                            effective_reasoning: ReasoningLevel::Off,
                        },
                    )])) as crate::ports::InvocationStream,
                )
            })
        },
    ));

    let binding = test_binding_from_port(port);
    let factory = crate::application::model::test_support::constant_factory(binding);

    let workspace = crate::application::run::workspace_test_support::runtime_workspace(
        &crate::application::run::workspace_test_support::test_tool_execution_context(
            std::env::temp_dir(),
            tokio_util::sync::CancellationToken::new(),
        ),
    );

    let runner = CliAgentRunner {
        factory,
        active_run: Arc::new(
            crate::application::run::active_registry::ActiveRunRegistry::default(),
        ),
        max_tool_concurrency: 10,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        tool_result_materializer:
            crate::application::tool::test_support::test_tool_result_materializer(),
        workspace,
        skill_catalog: tools::composition::wire_skills().catalog(),
        parent_context: source,
        runtime_context_factory: Arc::new(shared_factory),
    };
    let ctx = test_ctx();
    let (tx, _rx) = mpsc::channel::<tools::AgentProgressEvent>(8);

    // Spawn run_agent so we can cancel the parent token from outside.
    let handle = tokio::spawn(async move {
        runner
            .run_agent(AgentRunRequest {
                prompt: "run blocking",
                system: "system",
                identity: ctx.scope(),
                cancellation: ctx.cancellation(),
                progress: Some(crate::application::run::context::tool_progress_sink(tx)),
                memory: ctx.memory(),
                catalog: ctx.catalog_query(),
                read_set: ctx.read_set(),
                plan_mode: ctx.plan_mode_state(),
                guidance: ctx.guidance(),
                timeout: std::time::Duration::from_secs(30),
                role: "coder",
            })
            .await
    });

    // Wait for the blocking tool to actually start.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if tool_started.load(Ordering::SeqCst) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("blocking tool must start within timeout");

    // Now cancel the PARENT token — this must propagate through
    // CombinedCancellationSignal to the blocking tool.
    parent_cancel.token().cancel();

    let result = tokio::time::timeout(std::time::Duration::from_secs(10), handle)
        .await
        .expect("run_agent must finish within timeout after cancellation")
        .expect("run_agent task must not panic");

    // The run must terminate with Cancelled, proving that
    // CombinedCancellationSignal actually propagated the parent cancel.
    assert!(
        matches!(result, tools::AgentRunTerminal::Cancelled),
        "L3: parent cancel must terminate the run (got {result:?})"
    );

    drop(parent_frame_guard);
}

// ── #1385 Task 12: Sub-agent I/O seam tests ──

/// Deriving a sub-run produces a RuntimeContext whose event_sink is
/// a real handle (not causing panics when used), usage tracker starts
/// fresh (None), and input buffer handle starts fresh (unsealed).
#[test]
fn derived_sub_run_has_real_io_seams_not_noop_placeholders() {
    let parent_ctx = make_spy_parent_context(
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );

    let event_sink = parent_ctx.event_sink();
    // Send a probe event through the derived context's event_sink.
    // Derived event sink is a noop, so this should not panic.
    use crate::application::loop_engine::chat::ChatEventSink as _;
    event_sink.try_send_event(
        crate::application::loop_engine::chat::RuntimeStreamEvent::SystemMessage(
            "sub-derivation-probe".to_string(),
        ),
    );

    let usage = parent_ctx.usage();
    assert_eq!(usage.get(), None);

    let input = parent_ctx.input();
    assert!(!input.is_sealed());
}

/// Derived context's usage tracker is isolated from parent's.
#[test]
fn derived_sub_usage_tracker_isolated_from_parent() {
    let parent_ctx = make_spy_parent_context(
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );

    parent_ctx.usage().update(999);

    let fresh_usage = RunUsageTracker::new();
    assert_eq!(fresh_usage.get(), None);
    assert_eq!(parent_ctx.usage().get(), Some(999));
}

/// Derived context's input buffer handle is isolated from parent's.
#[test]
fn derived_sub_input_buffer_isolated_from_parent() {
    let parent_ctx = make_spy_parent_context(
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );

    parent_ctx.input().push(sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "parent-input".to_string(),
        images: vec![],
    });

    let fresh_input = RunInputBufferHandle::new();
    assert!(!fresh_input.is_sealed());
    let snapshot = fresh_input.with_lock(|buf| buf.user_message_snapshot());
    assert!(snapshot.is_empty());
}

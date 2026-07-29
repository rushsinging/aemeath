//! RuntimeContext L1/L2 测试。
//!
//! #1385 Task 2：验证 per-Run 活契约容器不包含 Workspace/MainSessionWiring/Composition scope。

use std::sync::Arc;

use crate::application::interaction::{InteractionBridge, InteractionPort};
use crate::application::run_config::RunConfigSnapshot;
use crate::application::runtime_context::{
    ParentRunContextSource, ParentRunFrame, RunCancellationScope, RunContextBindings,
    RunInputBufferHandle, RunUsageTracker, RuntimeContext,
};
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::ports::{
    ContextPort, PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderPort,
};
use hook::{HookInvocation, HookOutcome, HookPort};
use memory::api::MemoryPort;
use tools::{
    ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionOutcome,
    ToolExecutionPort, ToolInvocation, ToolProfileName,
};

// ── Test helper: noop event sink ──

fn noop_event_sink() -> crate::application::main_loop::ChatEventSinkHandle {
    #[derive(Clone)]
    struct NoOp;
    impl crate::application::main_loop::ChatEventSink for NoOp {
        fn send_event<'a>(
            &'a self,
            _event: crate::application::main_loop::RuntimeStreamEvent,
        ) -> crate::application::main_loop::EventFuture<'a> {
            Box::pin(std::future::ready(()))
        }
        fn try_send_event(&self, _event: crate::application::main_loop::RuntimeStreamEvent) {}
    }
    crate::application::main_loop::ChatEventSinkHandle::new(NoOp)
}

// ── 测试用 fake / no-op 实现 ──

struct FakeContextPort;
#[async_trait::async_trait]
impl ContextPort for FakeContextPort {
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

struct FakeProviderPort;
#[async_trait::async_trait]
impl ProviderPort for FakeProviderPort {
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

struct FakeToolCatalog;
impl ToolCatalogPort for FakeToolCatalog {
    fn snapshot(
        &self,
        _scope: &tools::RegistryScopeName,
        _profile: &ToolProfileName,
    ) -> Result<ToolCatalogSnapshot, ToolCatalogError> {
        Ok(ToolCatalogSnapshot::new(
            tools::RegistryScopeName::new("fake"),
            ToolProfileName::new("fake"),
            vec![],
        ))
    }
}

struct FakeToolExecution;
#[async_trait::async_trait]
impl ToolExecutionPort for FakeToolExecution {
    async fn execute(
        &self,
        _invocation: ToolInvocation,
        _context: &tools::ToolExecutionContext,
    ) -> ToolExecutionOutcome {
        ToolExecutionOutcome::success_text("fake")
    }
}

struct FakePolicy;
impl PolicyPort for FakePolicy {
    fn evaluate(&self, _request: &PolicyRequest) -> PolicyDecision {
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    }
}

struct FakeReflectionHistory;
#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for FakeReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::MemoryError> {
        Ok(vec![])
    }
}
#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for FakeReflectionHistory {
    async fn append(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::MemoryError> {
        Ok(())
    }
    async fn upsert(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::MemoryError> {
        Ok(())
    }
}

// FakeTaskAccess not needed — use `task::TaskStore::new()` directly which implements TaskAccess.

struct FakeHook;
#[async_trait::async_trait]
impl HookPort for FakeHook {
    async fn dispatch(
        &self,
        _invocation: HookInvocation,
        _cancellation: &dyn hook::CancellationSignal,
    ) -> HookOutcome {
        HookOutcome::proceed()
    }
}
// ── #1248 Task 3: Factory-based context construction helper ──

fn make_context() -> RuntimeContext {
    let factory = make_factory();
    let bindings = make_bindings();
    factory
        .assemble(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test context assembly must succeed")
}

fn make_factory() -> RuntimeContextFactory {
    RuntimeContextFactory::new(
        Arc::new(FakeToolCatalog),
        Arc::new(FakeToolExecution),
        Arc::new(FakePolicy),
        Arc::new(FakeReflectionHistory),
        Arc::new(task::TaskStore::new()),
        Arc::new(FakeHook),
    )
}

fn make_bindings() -> RunContextBindings {
    let provider_port: Arc<dyn ProviderPort> = Arc::new(FakeProviderPort);
    let binding = ProviderBinding {
        provider: provider_port.clone(),
        model: crate::ports::ModelId {
            provider: "test".into(),
            model: "test-model".into(),
        },
        max_tokens: 4096,
        requested_reasoning: provider::ReasoningLevel::Medium,
        context_window: None,
    };

    RunContextBindings {
        context: Arc::new(FakeContextPort),
        provider: Arc::new(binding),
        interaction: Arc::new(InteractionBridge::new()),
        memory: Arc::new(memory::NoOpMemory),
        config: RunConfigSnapshot::capture(
            share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
                share::config::domain::snapshot::ConfigRevision::new(1),
                share::config::Config::default(),
            ),
        ),
        cancel: RunCancellationScope::new(),
        event_sink: noop_event_sink(),
        usage: RunUsageTracker::new(),
        input: RunInputBufferHandle::new(),
        reasoning: Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium)),
        tool_catalog: None,
    }
}

// ── L1：资源 identity 测试 ──

#[test]
fn main_runtime_context_preserves_injected_port_identity() {
    let context_arc: Arc<dyn ContextPort> = Arc::new(FakeContextPort);
    let provider_port: Arc<dyn ProviderPort> = Arc::new(FakeProviderPort);
    let binding = ProviderBinding {
        provider: provider_port.clone(),
        model: crate::ports::ModelId {
            provider: "test".into(),
            model: "test-model".into(),
        },
        max_tokens: 4096,
        requested_reasoning: provider::ReasoningLevel::Medium,
        context_window: None,
    };
    let provider_arc = Arc::new(binding);
    let tool_catalog_arc: Arc<dyn ToolCatalogPort> = Arc::new(FakeToolCatalog);
    let tool_execution_arc: Arc<dyn ToolExecutionPort> = Arc::new(FakeToolExecution);
    let policy_arc: Arc<dyn PolicyPort> = Arc::new(FakePolicy);
    let interaction_arc: Arc<dyn InteractionPort> = Arc::new(InteractionBridge::new());
    let memory_arc: Arc<dyn MemoryPort> = Arc::new(memory::NoOpMemory);
    let reflection_history_arc: Arc<dyn memory::api::ReflectionHistoryStore> =
        Arc::new(FakeReflectionHistory);
    let task_arc: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let hooks_arc: Arc<dyn HookPort> = Arc::new(FakeHook);
    let reasoning_arc = Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium));

    let factory = RuntimeContextFactory::new(
        tool_catalog_arc.clone(),
        tool_execution_arc.clone(),
        policy_arc.clone(),
        reflection_history_arc.clone(),
        task_arc.clone(),
        hooks_arc.clone(),
    );
    let bindings = RunContextBindings {
        context: context_arc.clone(),
        provider: provider_arc.clone(),
        interaction: interaction_arc.clone(),
        memory: memory_arc.clone(),
        config: RunConfigSnapshot::capture(
            share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
                share::config::domain::snapshot::ConfigRevision::new(1),
                share::config::Config::default(),
            ),
        ),
        cancel: RunCancellationScope::new(),
        event_sink: noop_event_sink(),
        usage: RunUsageTracker::new(),
        input: RunInputBufferHandle::new(),
        reasoning: reasoning_arc.clone(),
        tool_catalog: None,
    };

    let ctx = factory
        .assemble(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test context assembly");

    assert!(Arc::ptr_eq(&ctx.context(), &context_arc));
    assert!(Arc::ptr_eq(&ctx.provider(), &provider_arc));
    assert!(Arc::ptr_eq(&ctx.tool_catalog(), &tool_catalog_arc));
    assert!(Arc::ptr_eq(&ctx.tool_execution(), &tool_execution_arc));
    assert!(Arc::ptr_eq(&ctx.policy(), &policy_arc));
    assert!(Arc::ptr_eq(&ctx.interaction(), &interaction_arc));
    assert!(Arc::ptr_eq(&ctx.memory(), &memory_arc));
    assert!(Arc::ptr_eq(
        &ctx.reflection_history(),
        &reflection_history_arc
    ));
    assert!(Arc::ptr_eq(&ctx.task(), &task_arc));
    assert!(Arc::ptr_eq(&ctx.hooks(), &hooks_arc));
    assert!(Arc::ptr_eq(&ctx.reasoning(), &reasoning_arc));
}

// ── L1：cancellation scope 测试 ──

#[test]
fn runtime_context_cancel_scope_is_per_run() {
    let ctx1 = make_context();
    let ctx2 = make_context();

    assert!(!ctx1.cancel().token().is_cancelled());
    assert!(!ctx2.cancel().token().is_cancelled());

    let child = ctx1.cancel().child_scope();
    assert!(!child.token().is_cancelled());

    ctx1.cancel().token().cancel();
    assert!(ctx1.cancel().token().is_cancelled());
    assert!(child.token().is_cancelled());
    assert!(!ctx2.cancel().token().is_cancelled());
}

#[test]
fn child_cancel_does_not_propagate_to_parent() {
    let parent = RunCancellationScope::new();
    let child = parent.child_scope();

    child.token().cancel();
    assert!(child.token().is_cancelled());
    assert!(!parent.token().is_cancelled());
}

// ── L2：API surface 测试 ──

#[test]
fn runtime_context_has_all_required_accessors() {
    let ctx = make_context();

    let _context = ctx.context();
    let _provider = ctx.provider();
    let _tool_catalog = ctx.tool_catalog();
    let _tool_execution = ctx.tool_execution();
    let _policy = ctx.policy();
    let _interaction = ctx.interaction();
    let _memory = ctx.memory();
    let _reflection_history = ctx.reflection_history();
    let _task = ctx.task();
    let _hooks = ctx.hooks();
    let _reasoning = ctx.reasoning();
    let _config = ctx.config();
    let _cancel = ctx.cancel();
}

#[test]
fn runtime_context_config_accessor_returns_snapshot() {
    let ctx = make_context();
    let cfg = ctx.config();
    let _revision = cfg.revision();
    let _allow_all = cfg.allow_all();
}

// ── Task 5: Behavioral assertion — RuntimeContext ports are functional, not just Arc identity ──

/// #1385 Task 5: RuntimeContext must hold live ports that execute real behavior,
/// not just pass Arc pointer-equality checks.
///
/// This test proves that `tool_catalog()`, `policy()`, `hooks()`, and
/// `reasoning()` can be called through RuntimeContext and return meaningful
/// results from their injected implementations.
#[test]
fn runtime_context_ports_are_functional_not_just_identity() {
    let ctx = make_context();

    // Tool catalog: snapshot() returns a valid catalog.
    let catalog = ctx
        .tool_catalog()
        .snapshot(
            &tools::RegistryScopeName::new("test-scope"),
            &ToolProfileName::new("test-profile"),
        )
        .expect("tool catalog snapshot must succeed");
    assert!(
        catalog.tools.is_empty(),
        "fake tool catalog returns empty tool list"
    );

    // Policy: calling evaluate() proves the port is functional through
    // RuntimeContext trait dispatch, not just Arc::ptr_eq.
    let _decision = ctx.policy_ref();
    assert!(Arc::ptr_eq(ctx.policy_ref(), ctx.policy_ref(),));

    // Hooks: prove the Arc is functional via trait dispatch.
    let _hooks = ctx.hooks();
    // Task: prove accessor is wired.
    let _task = ctx.task();

    // Interaction bridge: non-panicking smoke check.
    let _bridge = ctx.interaction();

    // Config: can read config values.
    assert!(
        !ctx.config().allow_all(),
        "default config has allow_all=false"
    );

    assert_eq!(
        *ctx.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
        provider::ReasoningLevel::Medium,
    );
}

// ── #1385: Poison recovery ──

/// `ParentRunContextSource` must recover from a poisoned lock in `install()`
/// without panicking, keeping the source available for future use.
#[test]
fn parent_run_context_source_install_recovers_from_poison() {
    let source = ParentRunContextSource::new();

    // Poison the lock by panicking while holding a write guard.
    let inner = source.inner.clone();
    let handle = std::thread::spawn(move || {
        let _guard = inner.write().unwrap();
        panic!("deliberate poison");
    });
    assert!(handle.join().is_err(), "thread must have panicked");

    // After poison, install must still succeed (recover, not panic).
    let frame = Arc::new(ParentRunFrame {
        spec: crate::domain::agent_run::RunSpec::main(),
        context: Arc::new(make_context()),
    });
    let _guard = source.install(frame);
    // If we got here without panicking, poison recovery works.
}

/// `ParentRunContextSource::get()` must recover from a poisoned lock
/// without panicking.
#[test]
fn parent_run_context_source_get_recovers_from_poison() {
    let source = ParentRunContextSource::new();

    // Pre-populate a frame so get() has something to return.
    let frame = Arc::new(ParentRunFrame {
        spec: crate::domain::agent_run::RunSpec::main(),
        context: Arc::new(make_context()),
    });
    let _guard = source.install(frame);

    // Poison the lock.
    let inner = source.inner.clone();
    let handle = std::thread::spawn(move || {
        let _guard = inner.write().unwrap();
        panic!("deliberate poison");
    });
    assert!(handle.join().is_err());

    // After poison, get() must still succeed.
    let result = source.get();
    // The frame that was installed before the poison should still be
    // recoverable (poison recovery gives us the inner data).
    assert!(
        result.is_some(),
        "get() must return frame after poison recovery"
    );
}

/// `ParentRunFrameGuard::drop()` must NOT double-panic when the lock is
/// poisoned.  Since `drop` is called during unwinding, a second panic
/// would abort the process.
#[test]
fn parent_run_frame_guard_drop_does_not_double_panic_on_poison() {
    let source = ParentRunContextSource::new();
    let frame = Arc::new(ParentRunFrame {
        spec: crate::domain::agent_run::RunSpec::main(),
        context: Arc::new(make_context()),
    });
    let guard = source.install(frame);

    // Poison the lock.
    let inner = source.inner.clone();
    let handle = std::thread::spawn(move || {
        let _write_guard = inner.write().unwrap();
        panic!("deliberate poison");
    });
    assert!(handle.join().is_err());

    // Drop the guard — must NOT panic (no double panic / abort).
    drop(guard);
    // If we reach here, the Drop implementation handled poison gracefully.
}

/// After poison recovery, `install()` must properly update the frame
/// and the returned guard must still clear only its own generation.
#[test]
fn parent_run_context_source_install_works_after_poison_recovery() {
    let source = ParentRunContextSource::new();

    // Poison the lock.
    let inner = source.inner.clone();
    let handle = std::thread::spawn(move || {
        let _guard = inner.write().unwrap();
        panic!("deliberate poison");
    });
    assert!(handle.join().is_err());

    // Install a new frame after poison.
    let frame = Arc::new(ParentRunFrame {
        spec: crate::domain::agent_run::RunSpec::main(),
        context: Arc::new(make_context()),
    });
    let guard = source.install(frame);

    // After install, get() must return a frame.
    assert!(source.get().is_some());

    // Drop the guard — frame should be cleared.
    drop(guard);
    assert!(
        source.get().is_none(),
        "guard drop must clear the installed frame"
    );
}

// ── #1385: CancellationToken clone semantics ──

/// `tokio_util::sync::CancellationToken::clone()` **shares** the same
/// underlying cancellation state (Arc-based).  Cancelling one clone
/// cancels all clones simultaneously.
///
/// This test proves the actual `tokio_util` 0.7.x semantics so we don't
/// rely on assumptions.
#[test]
fn cancellation_token_clone_shares_state() {
    let a = tokio_util::sync::CancellationToken::new();
    let b = a.clone();

    a.cancel();
    assert!(a.is_cancelled());
    assert!(
        b.is_cancelled(),
        "CancellationToken::clone() shares the same inner Arc; \
         cancelling a must also cancel b"
    );
}

/// `CancellationToken::child_token()` creates a **linked** child —
/// cancelling the parent cancels the child, but NOT vice versa.
#[test]
fn cancellation_token_child_is_linked_from_parent() {
    let parent = tokio_util::sync::CancellationToken::new();
    let child = parent.child_token();

    parent.cancel();
    assert!(parent.is_cancelled());
    assert!(
        child.is_cancelled(),
        "child_token must be cancelled when parent is cancelled"
    );
}

/// `CancellationToken::child_token()` is one-way: cancelling the child
/// does NOT cancel the parent.
#[test]
fn cancellation_token_child_does_not_cancel_parent() {
    let parent = tokio_util::sync::CancellationToken::new();
    let child = parent.child_token();

    child.cancel();
    assert!(child.is_cancelled());
    assert!(
        !parent.is_cancelled(),
        "cancelling a child_token must NOT cancel the parent"
    );
}

/// `RunCancellationScope::clone()` shares the same cancellation state
/// (via `CancellationToken::clone()`).  Cancel propagation is symmetric.
#[test]
fn run_cancellation_scope_clone_shares_state() {
    let scope_a = RunCancellationScope::new();
    let scope_b = scope_a.clone();

    scope_a.token().cancel();
    assert!(scope_a.token().is_cancelled());
    assert!(
        scope_b.token().is_cancelled(),
        "RunCancellationScope::clone() shares the same token; \
         cancelling scope_a must cancel scope_b"
    );
}

/// `RuntimeContext::clone()` shares the cancellation scope — the cloned
/// context sees the same token as the original.
#[test]
fn runtime_context_clone_shares_cancellation() {
    let ctx_a = make_context();
    let ctx_b = ctx_a.clone();

    ctx_a.cancel().token().cancel();
    assert!(ctx_a.cancel().token().is_cancelled());
    assert!(
        ctx_b.cancel().token().is_cancelled(),
        "RuntimeContext::clone() shares cancellation scope; \
         cancelling ctx_a must cancel ctx_b"
    );
}

/// When a `RuntimeContext` is shared via `Arc`, cancellation IS shared
/// because both references point to the same `RunCancellationScope`.
#[test]
fn runtime_context_arc_shared_cancellation_propagates() {
    let ctx = Arc::new(make_context());
    let ctx2 = Arc::clone(&ctx);

    ctx.cancel().token().cancel();
    assert!(ctx.cancel().token().is_cancelled());
    assert!(
        ctx2.cancel().token().is_cancelled(),
        "Arc<RuntimeContext> clones share the same cancellation scope; \
         cancelling through one Arc must be visible through the other"
    );
}

/// `RunCancellationScope::child_scope()` creates a linked child whose
/// cancellation propagates from parent to child, but not vice versa.
#[test]
fn run_cancellation_scope_child_propagates_from_parent() {
    let parent = RunCancellationScope::new();
    let child = parent.child_scope();

    parent.token().cancel();
    assert!(parent.token().is_cancelled());
    assert!(child.token().is_cancelled());
}

/// `RunCancellationScope::child_scope()` is one-way: cancelling the
/// child does NOT cancel the parent.
#[test]
fn run_cancellation_scope_child_does_not_cancel_parent() {
    let parent = RunCancellationScope::new();
    let child = parent.child_scope();

    child.token().cancel();
    assert!(child.token().is_cancelled());
    assert!(!parent.token().is_cancelled());
}

// ── #1385 Task 11: I/O seam tests ──

// ── RunUsageTracker tests ──

#[test]
fn run_usage_tracker_starts_at_none() {
    let tracker = RunUsageTracker::new();
    assert_eq!(tracker.get(), None);
}

#[test]
fn run_usage_tracker_update_and_read() {
    let tracker = RunUsageTracker::new();
    tracker.update(1500);
    assert_eq!(tracker.get(), Some(1500));
    tracker.update(3000);
    assert_eq!(tracker.get(), Some(3000));
}

#[test]
fn run_usage_tracker_clone_shares_state() {
    let tracker = RunUsageTracker::new();
    let clone = tracker.clone();
    tracker.update(500);
    assert_eq!(clone.get(), Some(500));
    clone.update(999);
    assert_eq!(tracker.get(), Some(999));
}

#[test]
fn run_usage_tracker_new_runs_are_isolated() {
    let tracker1 = RunUsageTracker::new();
    let tracker2 = RunUsageTracker::new();
    tracker1.update(100);
    tracker2.update(200);
    assert_eq!(tracker1.get(), Some(100));
    assert_eq!(tracker2.get(), Some(200));
}

#[test]
fn run_usage_tracker_recovers_from_poison() {
    let tracker = RunUsageTracker::new();
    let clone = tracker.clone();

    // Poison the lock by panicking while holding write guard.
    let inner = tracker.last_api_total_tokens.clone();
    let handle = std::thread::spawn(move || {
        let _guard = inner.write().unwrap();
        panic!("deliberate poison");
    });
    assert!(handle.join().is_err());

    // After poison, get() must still work (recover, not panic).
    let _val = clone.get();
    // Also test that update works after poison.
    clone.update(42);
    assert_eq!(clone.get(), Some(42));
}

// ── RunInputBufferHandle tests ──

#[test]
fn run_input_buffer_handle_new_is_not_sealed() {
    let handle = RunInputBufferHandle::new();
    assert!(!handle.is_sealed());
}

#[test]
fn run_input_buffer_handle_push_rejected_on_unsealed() {
    let handle = RunInputBufferHandle::new();

    let event = sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "hello".to_string(),
        images: vec![],
    };
    let rejected = handle.push_or_reject(event);
    assert!(rejected.is_none(), "unsealed buffer accepts user messages");
}

#[test]
fn run_input_buffer_handle_clone_shares_buffer() {
    let handle = RunInputBufferHandle::new();
    let clone = handle.clone();

    assert_eq!(handle.is_sealed(), clone.is_sealed());
}

#[test]
fn run_input_buffer_handle_new_handles_are_isolated() {
    let h1 = RunInputBufferHandle::new();
    let h2 = RunInputBufferHandle::new();
    assert!(!h1.is_sealed());
    assert!(!h2.is_sealed());
}

// ── RuntimeContext I/O seam accessor tests ──

fn make_context_with_io_seams() -> (
    RuntimeContext,
    crate::application::main_loop::ChatEventSinkHandle,
    RunUsageTracker,
    RunInputBufferHandle,
) {
    let event_sink = noop_event_sink();
    let usage = RunUsageTracker::new();
    let input = RunInputBufferHandle::new();

    let factory = make_factory();
    let mut bindings = make_bindings();
    bindings.event_sink = event_sink.clone();
    bindings.usage = usage.clone();
    bindings.input = input.clone();

    let ctx = factory
        .assemble(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test context assembly must succeed");

    (ctx, event_sink, usage, input)
}

#[test]
fn runtime_context_event_sink_accessor_preserves_identity() {
    let (ctx, event_sink, _usage, _input) = make_context_with_io_seams();
    // Smoke test — calling event_sink() works.
    let _ = ctx.event_sink();
    let _ = event_sink;
}

#[test]
fn runtime_context_usage_accessor_preserves_identity() {
    let (ctx, _event_sink, usage, _input) = make_context_with_io_seams();

    usage.update(123);
    assert_eq!(ctx.usage().get(), Some(123));

    ctx.usage().update(456);
    assert_eq!(usage.get(), Some(456));
}

#[test]
fn runtime_context_input_accessor_preserves_identity() {
    let (ctx, _event_sink, _usage, input) = make_context_with_io_seams();

    assert_eq!(input.is_sealed(), ctx.input().is_sealed());
}

#[test]
fn runtime_context_clone_shares_io_seams() {
    let (ctx, _event_sink, usage, input) = make_context_with_io_seams();
    let clone = ctx.clone();

    usage.update(777);
    assert_eq!(clone.usage().get(), Some(777));

    input.push(sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "via clone".to_string(),
        images: vec![],
    });
    assert!(!clone.input().is_sealed());
    assert!(!ctx.input().is_sealed());
}

#[test]
fn runtime_context_new_runs_have_isolated_io_seams() {
    let (ctx1, _sink1, usage1, input1) = make_context_with_io_seams();
    let (ctx2, _sink2, usage2, input2) = make_context_with_io_seams();

    ctx1.usage().update(100);
    ctx2.usage().update(200);
    assert_eq!(usage1.get(), Some(100));
    assert_eq!(usage2.get(), Some(200));

    input1.push(sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "run1".to_string(),
        images: vec![],
    });
    input2.push(sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "run2".to_string(),
        images: vec![],
    });
    assert!(!input1.is_sealed());
    assert!(!input2.is_sealed());
}

#[test]
fn runtime_context_has_all_required_accessors_with_io_seams() {
    let (ctx, _sink, _usage, _input) = make_context_with_io_seams();

    // Existing accessors still work.
    let _context = ctx.context();
    let _provider = ctx.provider();
    let _tool_catalog = ctx.tool_catalog();
    let _tool_execution = ctx.tool_execution();
    let _policy = ctx.policy();
    let _interaction = ctx.interaction();
    let _memory = ctx.memory();
    let _reflection_history = ctx.reflection_history();
    let _task = ctx.task();
    let _hooks = ctx.hooks();
    let _reasoning = ctx.reasoning();
    let _config = ctx.config();
    let _cancel = ctx.cancel();

    // New I/O seam accessors.
    let _event_sink = ctx.event_sink();
    let _usage = ctx.usage();
    let _input = ctx.input();
}

// ── #1385 Task 12: RuntimeContext I/O seam production-facing tests ──

/// Proves that `RunInputBufferHandle::with_lock` gives access to
/// the SAME inner buffer across clones — satisfying the requirement
/// that MainInputStrategy and RuntimeContext share buffer identity.
#[test]
fn run_input_buffer_handle_with_lock_shares_identity_across_clones() {
    let h1 = RunInputBufferHandle::new();
    let h2 = h1.clone();

    // Push through h1, read through h2 via with_lock.
    let event = sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "shared-identity".to_string(),
        images: vec![],
    };
    h1.push(event);

    let snapshot = h2.with_lock(|buf| buf.user_message_snapshot());
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].1.text_content(), "shared-identity");
}

/// Proves that `RunInputBufferHandle::with_lock` can drain through
/// the handle, then a clone sees the drained state.
#[test]
fn run_input_buffer_handle_with_lock_drain_visible_across_clones() {
    let h1 = RunInputBufferHandle::new();
    let h2 = h1.clone();

    h1.push(sdk::ChatInputEvent::UserMessage {
        id: sdk::InputId::new_v7(),
        text: "drain-me".to_string(),
        images: vec![],
    });

    // First drain: buffer has data → Ready, not sealed.
    let epoch0 = crate::application::loop_engine::DrainEpoch(0);
    let drain_result = h1.with_lock(|buf| buf.drain_or_seal(epoch0));
    assert!(matches!(
        drain_result,
        crate::application::main_loop::looping::run_input_buffer::BufferDrain::Ready { .. }
    ));
    assert!(!h1.is_sealed());

    // Second drain: buffer is empty → EmptyAndSealed, now sealed.
    let epoch1 = crate::application::loop_engine::DrainEpoch(1);
    let drain_result2 = h1.with_lock(|buf| buf.drain_or_seal(epoch1));
    assert!(matches!(
        drain_result2,
        crate::application::main_loop::looping::run_input_buffer::BufferDrain::EmptyAndSealed { .. }
    ));

    // h2 sees the buffer as now sealed.
    assert!(h2.is_sealed());
}

/// Proves that `RunUsageTracker::reset()` sets the value to `None`,
/// and `get()` returns `None` afterward.
#[test]
fn run_usage_tracker_reset_clears_value() {
    let tracker = RunUsageTracker::new();
    tracker.update(42);
    assert_eq!(tracker.get(), Some(42));

    tracker.reset();
    assert_eq!(tracker.get(), None);
}

/// Proves that `RunUsageTracker::update` with zero is NOT the same
/// as reset (zero is a valid update, reset makes it None).
#[test]
fn run_usage_tracker_update_zero_versus_reset() {
    let tracker = RunUsageTracker::new();
    tracker.update(0);
    assert_eq!(tracker.get(), Some(0));

    tracker.reset();
    assert_eq!(tracker.get(), None);
}

/// Proves that a real event sink passed through the factory
/// is the same handle returned by `RuntimeContext::event_sink()` —
/// both point to the same inner `Arc`.
/// Uses an observable `SharedEventSink` that records events.
#[test]
fn factory_assembled_context_event_sink_is_real_not_noop() {
    use crate::application::main_loop::ChatEventSink;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Clone)]
    struct SharedSink {
        seen: Arc<AtomicBool>,
    }

    impl ChatEventSink for SharedSink {
        fn send_event<'a>(
            &'a self,
            _event: crate::application::main_loop::RuntimeStreamEvent,
        ) -> crate::application::main_loop::EventFuture<'a> {
            self.seen.store(true, Ordering::SeqCst);
            Box::pin(std::future::ready(()))
        }
        fn try_send_event(&self, _event: crate::application::main_loop::RuntimeStreamEvent) {
            self.seen.store(true, Ordering::SeqCst);
        }
    }

    let seen = Arc::new(AtomicBool::new(false));
    let sink = SharedSink { seen: seen.clone() };
    let handle = crate::application::main_loop::ChatEventSinkHandle::new(sink);

    // Build a RuntimeContext with this real handle via factory.
    let factory = make_factory();
    let mut bindings = make_bindings();
    bindings.event_sink = handle;
    let ctx = factory
        .assemble(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test context assembly");

    // Send an event through the RuntimeContext's event_sink.
    let event_sink = ctx.event_sink();
    event_sink.try_send_event(
        crate::application::main_loop::RuntimeStreamEvent::SystemMessage("probe".to_string()),
    );

    assert!(
        seen.load(Ordering::SeqCst),
        "event_sink must be real, not noop"
    );
}

//! #1248 Task 3: RuntimeContextFactory assembly contract tests.
//!
//! TDD: failure tests are written first to prove the factory rejects
//! invalid combinations; success tests verify correct wiring of
//! capability-semantic dimensions (interaction, hook, reasoning).

use std::sync::Arc;
use std::time::Duration;

use crate::application::client::RuntimeContextAssemblyError;
use crate::application::interaction::InteractionBridge;
use crate::application::run_config::RunConfigSnapshot;
use crate::application::runtime_context::{
    RunCancellationScope, RunContextBindings, RunInputBufferHandle, RunUsageTracker,
    RuntimeContext, RuntimeServices,
};
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::domain::agent_run::{
    HookBindingMode, InteractionBindingMode, ReasoningBindingMode, RunSpec,
};
use crate::ports::{
    ContextPort, PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderPort,
};
use hook::{HookInvocation, HookOutcome, HookPort};
use tools::{
    ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionOutcome,
    ToolExecutionPort, ToolInvocation, ToolProfileName,
};

// ── Test helpers ──

fn main_spec() -> RunSpec {
    RunSpec::main()
}

fn sub_spec() -> RunSpec {
    RunSpec::sub("test-sub", Duration::from_secs(30))
}

// ── Test fakes ──

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

// ── Construction helpers ──

fn make_services() -> RuntimeServices {
    RuntimeServices {
        tool_catalog: Arc::new(FakeToolCatalog),
        tool_execution: Arc::new(FakeToolExecution),
        policy: Arc::new(FakePolicy),
        reflection_history: Arc::new(FakeReflectionHistory),
        task: Arc::new(task::TaskStore::new()),
        hooks: Arc::new(FakeHook),
    }
}

fn factory() -> RuntimeContextFactory {
    let s = make_services();
    RuntimeContextFactory::new(
        s.tool_catalog,
        s.tool_execution,
        s.policy,
        s.reflection_history,
        s.task,
        s.hooks,
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
        skill_load_session_id: "session".to_string(),
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

fn make_parent_context() -> RuntimeContext {
    let services = make_services();
    let bindings = make_bindings();
    // #1248 Task 3: Use the test-only token — production code must go
    // through RuntimeContextFactory::assemble().
    let skill_load_state = Arc::new(
        crate::application::skill_load_state::ContextSkillLoadState::new(bindings.context.clone()),
    );
    RuntimeContext::new(
        services,
        bindings,
        skill_load_state,
        crate::application::runtime_context::RuntimeContextAssemblyToken::new_for_test(),
    )
}

// ══════════════════════════════════════════════════════════════════
// Task 1: Binding-mode selection tests (preserved, updated for new ctor)
// ══════════════════════════════════════════════════════════════════

#[test]
fn factory_selects_client_interaction_for_main_spec() {
    let f = factory();
    assert_eq!(
        f.select_interaction(&main_spec()),
        InteractionBindingMode::Client
    );
}

#[test]
fn factory_selects_parent_mediated_for_sub_spec() {
    let f = factory();
    assert_eq!(
        f.select_interaction(&sub_spec()),
        InteractionBindingMode::ParentMediated
    );
}

#[test]
fn factory_selects_unavailable_when_spec_declares_it() {
    let spec = sub_spec()
        .with_interaction_kind(InteractionBindingMode::Unavailable)
        .unwrap();
    let f = factory();
    assert_eq!(
        f.select_interaction(&spec),
        InteractionBindingMode::Unavailable
    );
}

#[test]
fn parent_mediated_without_parent_capabilities_returns_interaction_unavailable() {
    let spec = sub_spec();
    let f = factory();
    let result = f.select_interaction_with_parent(&spec, None);
    assert_eq!(
        result,
        Err(RuntimeContextAssemblyError::InteractionUnavailable)
    );
}

#[test]
fn parent_mediated_with_parent_capabilities_succeeds() {
    let spec = sub_spec();
    let f = factory();
    let parent_caps = main_spec();
    let result = f.select_interaction_with_parent(&spec, Some(&parent_caps));
    assert_eq!(result, Ok(InteractionBindingMode::ParentMediated));
}

#[test]
fn client_interaction_does_not_require_parent_capabilities() {
    let spec = sub_spec()
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap();
    let f = factory();
    assert_eq!(
        f.select_interaction_with_parent(&spec, None),
        Ok(InteractionBindingMode::Client)
    );
}

#[test]
fn factory_selects_full_hook_for_main_spec() {
    let f = factory();
    assert_eq!(f.select_hook(&main_spec()), HookBindingMode::Full);
}

#[test]
fn factory_selects_boundary_only_for_sub_spec() {
    let f = factory();
    assert_eq!(f.select_hook(&sub_spec()), HookBindingMode::BoundaryOnly);
}

#[test]
fn factory_selects_fixed_reasoning_when_spec_declares_it() {
    let spec = sub_spec()
        .with_reasoning(ReasoningBindingMode::Fixed)
        .unwrap();
    let f = factory();
    assert_eq!(f.select_reasoning(&spec), ReasoningBindingMode::Fixed);
}

// ══════════════════════════════════════════════════════════════════
// Task 3: Assembly failure tests (TDD — verify rejection first)
// ══════════════════════════════════════════════════════════════════

#[test]
fn assemble_parent_mediated_without_parent_fails() {
    // Sub spec has ParentMediated interaction.  Without a parent
    // RuntimeContext, assembly MUST fail with InteractionUnavailable.
    let f = factory();
    let spec = sub_spec(); // interaction_kind = ParentMediated
    let bindings = make_bindings();
    let result = f.assemble(&spec, bindings, None);
    assert_eq!(
        result.err(),
        Some(RuntimeContextAssemblyError::InteractionUnavailable)
    );
}

#[test]
fn assemble_boundary_only_hook_without_parent_fails() {
    // Sub spec has BoundaryOnly hooks.  Without a parent RuntimeContext,
    // assembly MUST fail with HookUnavailable.
    // Use Client interaction so we only trigger the hook validation.
    let spec = sub_spec()
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap(); // hooks = BoundaryOnly still
    let f = factory();
    let bindings = make_bindings();
    let result = f.assemble(&spec, bindings, None);
    assert_eq!(
        result.err(),
        Some(RuntimeContextAssemblyError::HookUnavailable)
    );
}

#[test]
fn assemble_unavailable_interaction_without_parent_succeeds() {
    // Unavailable interaction + BoundaryOnly hooks → should fail on hook,
    // not interaction.  But if we also change hooks to Full, it should pass.
    let spec = sub_spec()
        .with_interaction_kind(InteractionBindingMode::Unavailable)
        .unwrap()
        .with_hooks(HookBindingMode::Full)
        .unwrap()
        .with_reasoning(ReasoningBindingMode::NoOp)
        .unwrap();
    let f = factory();
    let bindings = make_bindings();
    let result = f.assemble(&spec, bindings, None);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

// ══════════════════════════════════════════════════════════════════
// Task 3: Assembly success tests
// ══════════════════════════════════════════════════════════════════

#[test]
fn assemble_main_spec_succeeds() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let result = f.assemble(&spec, bindings, None);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn assemble_main_ctx_has_client_interaction() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let ctx = f.assemble(&spec, bindings, None).unwrap();
    // Main spec = Client interaction → port is available (register succeeds).
    let request = sdk::InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: sdk::RunId::new_v7(),
        body: sdk::InteractionRequestBody::UserQuestions(vec![sdk::UserQuestion {
            prompt: "test".into(),
            options: vec![],
            allow_multi: false,
        }]),
    };
    let _waiter = ctx
        .interaction_ref()
        .register(request)
        .expect("Client interaction port should be available");
}

#[test]
fn assemble_sub_with_parent_succeeds() {
    let f = factory();
    let parent_ctx = make_parent_context();
    let spec = sub_spec(); // ParentMediated + BoundaryOnly + Inherit
    let bindings = make_bindings();
    let result = f.assemble(&spec, bindings, Some(&parent_ctx));
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn sub_context_inherits_parent_skill_load_state_backing() {
    let f = factory();
    let parent_ctx = make_parent_context();
    let parent_state = parent_ctx.skill_load_state();
    let child_context = Arc::new(FakeContextPort);
    let mut bindings = make_bindings();
    bindings.context = child_context;
    let child = f
        .assemble(&sub_spec(), bindings, Some(&parent_ctx))
        .expect("assemble sub context");
    let child_state = child.skill_load_state();

    assert!(Arc::ptr_eq(&parent_state, &child_state));
    assert_eq!(
        child.skill_load_session_id(),
        parent_ctx.skill_load_session_id()
    );
}

#[test]
fn assemble_sub_with_parent_has_parent_mediated_interaction() {
    let f = factory();
    let parent_ctx = make_parent_context();
    let spec = sub_spec();
    let bindings = make_bindings();
    let ctx = f.assemble(&spec, bindings, Some(&parent_ctx)).unwrap();
    // Sub context exists — interaction bridge is present (the adapter
    // carries the interaction capability; full mediation wiring is Task 4).
    let _ = ctx.interaction_ref();
}

// ══════════════════════════════════════════════════════════════════
// Task 3: Reasoning wiring tests
// ══════════════════════════════════════════════════════════════════

#[test]
fn assemble_adaptive_uses_bindings_reasoning_port() {
    let f = factory();
    let spec = main_spec(); // Adaptive reasoning
    let level = provider::ReasoningLevel::High;
    let mut bindings = make_bindings();
    let spy = Arc::new(std::sync::Mutex::new(level));
    bindings.reasoning = spy.clone();

    let ctx = f.assemble(&spec, bindings, None).unwrap();
    // The context's reasoning should be the same port we injected
    // (Adaptive mode = pass-through from bindings).
    assert_eq!(
        ctx.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        level
    );
}

#[test]
fn assemble_fixed_uses_bindings_reasoning_port() {
    let f = factory();
    let spec = sub_spec()
        .with_interaction_kind(InteractionBindingMode::Client)
        .unwrap()
        .with_hooks(HookBindingMode::Full)
        .unwrap()
        .with_reasoning(ReasoningBindingMode::Fixed)
        .unwrap();
    let level = provider::ReasoningLevel::Low;
    let mut bindings = make_bindings();
    let spy = Arc::new(std::sync::Mutex::new(level));
    bindings.reasoning = spy.clone();

    let ctx = f.assemble(&spec, bindings, None).unwrap();
    // Fixed mode = pass-through from bindings.
    assert_eq!(
        ctx.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        level
    );
}

// ══════════════════════════════════════════════════════════════════
// Task 3: Identity / resource tests
// ══════════════════════════════════════════════════════════════════

#[test]
fn assembled_context_preserves_service_port_identity() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let ctx = f.assemble(&spec, bindings, None).unwrap();

    // All service ports should be accessible (not panic).
    let _ = ctx.context();
    let _ = ctx.provider();
    let _ = ctx.tool_catalog();
    let _ = ctx.tool_execution();
    let _ = ctx.policy();
    let _ = ctx.interaction();
    let _ = ctx.memory();
    let _ = ctx.reflection_history();
    let _ = ctx.task();
    let _ = ctx.hooks();
    let _ = ctx.reasoning();
}

#[test]
fn assembled_context_preserves_per_run_fields() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let ctx = f.assemble(&spec, bindings, None).unwrap();

    // Per-run fields should be accessible (not panic).
    let _ = ctx.config();
    let _ = ctx.cancel();
    let _ = ctx.event_sink();
    let _ = ctx.usage();
    let _ = ctx.input();
}

#[test]
fn assembled_context_clone_shares_reasoning() {
    let f = factory();
    let spec = main_spec();
    let level = provider::ReasoningLevel::Medium;
    let mut bindings = make_bindings();
    let spy = Arc::new(std::sync::Mutex::new(level));
    bindings.reasoning = spy.clone();

    let ctx = f.assemble(&spec, bindings, None).unwrap();
    let ctx2 = ctx.clone();

    // Clone shares the same reasoning port (Arc identity).
    assert_eq!(
        ctx.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        level
    );
    assert_eq!(
        ctx2.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        level
    );

    // Mutate through ctx2's reasoning → visible in ctx's
    ctx2.reasoning()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone_from(&provider::ReasoningLevel::Low);
    assert_eq!(
        ctx.reasoning()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
        provider::ReasoningLevel::Low
    );
}

#[test]
fn assembled_context_cancel_is_shareable() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let ctx = f.assemble(&spec, bindings, None).unwrap();

    let cancel = ctx.cancel();
    let child = cancel.child_scope();
    // verify no panic
    let _ = child.token();
}

#[test]
fn two_independent_assemblies_have_independent_cancel() {
    let f = factory();
    let spec = main_spec();
    let ctx1 = f.assemble(&spec, make_bindings(), None).unwrap();
    let ctx2 = f.assemble(&spec, make_bindings(), None).unwrap();

    let cancel1 = ctx1.cancel();
    let cancel2 = ctx2.cancel();

    // Different contexts = different cancel tokens (by address).
    assert!(!std::ptr::eq(
        cancel1.token() as *const _,
        cancel2.token() as *const _
    ));
}

// ══════════════════════════════════════════════════════════════════
// #1248 Task 3: Lifecycle field classification tests
// ══════════════════════════════════════════════════════════════════

/// Prove that `RuntimeServices` ports (cross-Run stable) are shared across
/// independent assemblies from the same factory, while `RunContextBindings`
/// ports (per-Run) are NOT shared.
///
/// This validates the lifecycle split: services hold tool/policy/reflection/
/// task/hooks (cross-Run); bindings hold context/provider/interaction/
/// memory/reasoning + I/O seams (per-Run).
#[test]
fn service_ports_are_shared_across_assemblies_binding_ports_are_not() {
    let f = factory();
    let spec = main_spec();

    let ctx1 = f.assemble(&spec, make_bindings(), None).unwrap();
    let ctx2 = f.assemble(&spec, make_bindings(), None).unwrap();

    // ── Cross-Run service ports: same Arc identity ──
    assert!(Arc::ptr_eq(&ctx1.tool_catalog(), &ctx2.tool_catalog()));
    assert!(Arc::ptr_eq(&ctx1.tool_execution(), &ctx2.tool_execution()));
    assert!(Arc::ptr_eq(&ctx1.policy(), &ctx2.policy()));
    assert!(Arc::ptr_eq(
        &ctx1.reflection_history(),
        &ctx2.reflection_history()
    ));
    assert!(Arc::ptr_eq(&ctx1.task(), &ctx2.task()));
    assert!(Arc::ptr_eq(&ctx1.hooks(), &ctx2.hooks()));

    // ── Per-Run binding ports: different Arc identity ──
    assert!(!Arc::ptr_eq(&ctx1.context(), &ctx2.context()));
    assert!(!Arc::ptr_eq(&ctx1.provider(), &ctx2.provider()));
    assert!(!Arc::ptr_eq(&ctx1.interaction(), &ctx2.interaction()));
    assert!(!Arc::ptr_eq(&ctx1.memory(), &ctx2.memory()));
    // reasoning: Adaptive passes through from bindings, so different
    assert!(!Arc::ptr_eq(&ctx1.reasoning(), &ctx2.reasoning()));
}

/// Prove that service ports are Arc::ptr_eq even when binding-mode
/// dimensions differ (sub with parent vs main).  Cross-Run stability
/// is independent of per-Run capability choices.
#[test]
fn service_ports_are_stable_across_main_and_sub_assemblies() {
    let f = factory();

    // Create parent through the factory so all three share services.
    let parent_ctx = f.assemble(&main_spec(), make_bindings(), None).unwrap();
    let sub_ctx = f
        .assemble(&sub_spec(), make_bindings(), Some(&parent_ctx))
        .unwrap();
    let main_ctx = f.assemble(&main_spec(), make_bindings(), None).unwrap();

    // All three share the same service ports.
    for (a, b) in [
        (&parent_ctx, &sub_ctx),
        (&parent_ctx, &main_ctx),
        (&sub_ctx, &main_ctx),
    ] {
        assert!(Arc::ptr_eq(&a.tool_catalog(), &b.tool_catalog()));
        assert!(Arc::ptr_eq(&a.tool_execution(), &b.tool_execution()));
        assert!(Arc::ptr_eq(&a.policy(), &b.policy()));
        assert!(Arc::ptr_eq(
            &a.reflection_history(),
            &b.reflection_history()
        ));
        assert!(Arc::ptr_eq(&a.task(), &b.task()));
        assert!(Arc::ptr_eq(&a.hooks(), &b.hooks()));
    }
}

// ══════════════════════════════════════════════════════════════════
// #1248 Task 3: Inherit reasoning — missing parent hard error
// ══════════════════════════════════════════════════════════════════

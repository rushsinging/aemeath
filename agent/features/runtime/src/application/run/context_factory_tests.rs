//! #1248 Task 3: RuntimeContextFactory assembly contract tests.
//!
//! TDD: failure tests are written first to prove the factory rejects
//! invalid combinations; success tests verify correct wiring of
//! capability-semantic dimensions (interaction, hook, reasoning).

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::application::client::RuntimeContextAssemblyError;
use crate::application::interaction::port::InteractionBridge;
use crate::application::run::config::RunConfigSnapshot;
use crate::application::run::context::{
    RunCancellationScope, RunCapabilityBindings, RunInputBufferHandle, RunUsageTracker,
    RuntimeContext, RuntimeServices,
};
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::preparation::{RunPreparationRequest, SessionState};
use crate::application::run::preparer::RunPreparer;
use crate::domain::agent_run::{
    HookBindingMode, InteractionBindingMode, ReasoningBindingMode, RunSpec,
};
use crate::ports::{
    ContextPort, PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderPort,
};
use hook::{HookInvocation, HookOutcome, HookPort};
use tools::{
    ToolCatalogError, ToolCatalogPort, ToolCatalogSnapshot, ToolExecutionContextBindingPort,
    ToolExecutionOutcome, ToolExecutionPort, ToolInvocation, ToolProfileName,
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
        _cancellation: &dyn tools::CancellationSignal,
    ) -> ToolExecutionOutcome {
        ToolExecutionOutcome::success_text("fake")
    }
}

struct FakeToolContextBinding;
impl ToolExecutionContextBindingPort for FakeToolContextBinding {
    fn bind(&self, _context: tools::ToolExecutionContext) -> Result<(), String> {
        Ok(())
    }
    fn unbind(&self, _run_id: &str) {}
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
        _cancellation: &CancellationToken,
    ) -> HookOutcome {
        HookOutcome::proceed()
    }
}

/// A reasoning port whose `current_requested_level` can be externally set
/// for verification in tests that check which port was wired.
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

// ── Construction helpers ──

fn make_services() -> RuntimeServices {
    RuntimeServices {
        tool_catalog: Arc::new(FakeToolCatalog),
        tool_execution: Arc::new(FakeToolExecution),
        tool_context_binding: Arc::new(FakeToolContextBinding),
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
        s.tool_context_binding,
        s.policy,
        s.reflection_history,
        s.task,
        s.hooks,
    )
}

fn make_bindings() -> RunCapabilityBindings {
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

    RunCapabilityBindings {
        model: crate::application::run::context::ModelBindings {
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
            reasoning: Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium)),
            tool_catalog: None,
        },
        io: crate::application::run::context::IoBindings {
            event_sink: noop_event_sink(),
            input: RunInputBufferHandle::new(),
        },
        lifecycle: crate::application::run::context::LifecycleBindings {
            cancel: RunCancellationScope::new(),
            usage: RunUsageTracker::new(),
        },
    }
}

fn make_parent_context() -> RuntimeContext {
    let services = make_services();
    let bindings = make_bindings();
    // #1248 Task 3: Use the test-only token — production code must go
    // through RuntimeContextFactory::assemble().
    RuntimeContext::new(
        services,
        bindings,
        crate::application::run::context::RuntimeContextAssemblyToken::new_for_test(),
    )
}

#[test]
fn run_preparer_accepts_only_the_pure_value_request() {
    let source = include_str!("preparer.rs");
    let signature = source
        .split("pub fn prepare(")
        .nth(1)
        .and_then(|tail| tail.split(") -> Result<PreparedRun").next())
        .expect("RunPreparer::prepare signature");

    assert!(signature.contains("request: RunPreparationRequest"));
    assert!(!signature.contains("RunCapabilityBindings"));
    assert!(!signature.contains("RuntimeContext"));
    assert!(!signature.contains("parent:"));
}

#[test]
fn production_run_callers_do_not_assemble_context_bindings() {
    let main_source = include_str!("../loop_engine/chat/loop_runner.rs");
    let sub_source = include_str!("../run/derived/setup.rs");

    for source in [main_source, sub_source] {
        assert!(!source.contains("RunCapabilityBindings"));
        assert!(!source.contains("RunContextBindings"));
        assert!(!source.contains("RuntimeContextParts"));
        assert!(!source.contains(".assemble("));
    }
}

#[test]
fn runtime_context_construction_is_factory_private() {
    let context_source = include_str!("context.rs");
    let factory_source = include_str!("context_factory.rs");

    assert!(!context_source.contains("pub fn new(\n        services: RuntimeServices"));
    assert!(!factory_source.contains("pub fn assemble("));
    assert!(!factory_source.contains("pub fn create("));
}

#[test]
fn run_preparer_coordinates_context_factory_and_complete_prepared_run() {
    let factory = Arc::new(factory());
    struct TestMainResolver {
        bindings: RunCapabilityBindings,
    }
    impl crate::application::run::context_factory::RuntimeContextResolver for TestMainResolver {
        fn resolve(
            &self,
            factory: &RuntimeContextFactory,
            request: &RunPreparationRequest,
        ) -> Result<
            (
                RuntimeContext,
                crate::application::run::preparation::SessionSnapshot,
            ),
            crate::application::run::preparation::RunPreparationError,
        > {
            factory
                .create(request.spec(), self.bindings.clone(), None)
                .map(|context| (context, request.session().clone()))
                .map_err(|_| {
                    crate::application::run::preparation::RunPreparationError::ContextAssembly
                })
        }
    }
    let resolver = Arc::new(TestMainResolver {
        bindings: make_bindings(),
    });
    let preparer = RunPreparer::new(factory.clone(), resolver);
    let session = SessionState::new(
        "session-1",
        std::path::PathBuf::from("/workspace"),
        "test/test-model",
        share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config::default()),
    );
    let request =
        RunPreparationRequest::new(main_spec(), session.snapshot_for_run(), None).unwrap();

    let prepared = preparer.prepare(request).expect("prepare complete run");

    assert_eq!(
        prepared.run().status(),
        crate::domain::agent_run::RunStatus::Created
    );
    assert!(prepared.execution().messages().is_empty());
    assert!(prepared.context().is_some());
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

#[test]
fn grouped_capability_bindings_preserve_values() {
    let grouped = make_bindings();

    assert_eq!(grouped.model.provider.model.model, "test-model");
    assert!(grouped.model.tool_catalog.is_none());
    assert_eq!(grouped.lifecycle.usage.get(), None);
}

#[test]
fn sub_run_preparer_preserves_parent_identity_and_assembles_context() {
    let context_factory = Arc::new(factory());
    let parent_context = make_parent_context();
    let parent_spec = main_spec();
    let parent_run_id = crate::domain::agent_run::RunId::new_v7();
    let session = SessionState::new(
        "session-sub",
        std::path::PathBuf::from("/workspace/sub"),
        "test-provider/test-model",
        share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
            share::config::domain::snapshot::ConfigRevision::new(1),
            share::config::Config::default(),
        ),
    );
    let spec = parent_spec
        .derive_sub("coder", std::time::Duration::from_secs(30))
        .unwrap();
    let request = RunPreparationRequest::new(
        spec.clone(),
        session.snapshot_for_run(),
        Some(
            crate::application::run::preparation::ParentRunCapabilities::new(
                parent_run_id.clone(),
                parent_spec,
            ),
        ),
    )
    .unwrap();
    struct TestSubResolver {
        bindings: RunCapabilityBindings,
        parent: Arc<RuntimeContext>,
    }
    impl crate::application::run::context_factory::RuntimeContextResolver for TestSubResolver {
        fn resolve(
            &self,
            factory: &RuntimeContextFactory,
            request: &RunPreparationRequest,
        ) -> Result<
            (
                RuntimeContext,
                crate::application::run::preparation::SessionSnapshot,
            ),
            crate::application::run::preparation::RunPreparationError,
        > {
            factory
                .create(
                    request.spec(),
                    self.bindings.clone(),
                    Some(self.parent.as_ref()),
                )
                .map(|context| (context, request.session().clone()))
                .map_err(|_| {
                    crate::application::run::preparation::RunPreparationError::ContextAssembly
                })
        }
    }
    let resolver = Arc::new(TestSubResolver {
        bindings: make_bindings(),
        parent: Arc::new(parent_context),
    });
    let preparer = RunPreparer::new(context_factory, resolver);

    let prepared = preparer.prepare(request).unwrap();

    assert_eq!(prepared.run().spec(), &spec);
    assert_eq!(prepared.run().parent_id(), Some(&parent_run_id));
    assert!(prepared.context().is_some());
    assert!(prepared.execution().messages().is_empty());
}

#[test]
fn sub_run_preparer_rejects_missing_parent_context() {
    let context_factory = Arc::new(factory());
    let parent_spec = main_spec();
    let parent_run_id = crate::domain::agent_run::RunId::new_v7();
    let session = SessionState::new(
        "session-sub",
        std::path::PathBuf::from("/workspace/sub"),
        "test-provider/test-model",
        share::config::domain::snapshot::ConfigSnapshot::new_with_revision(
            share::config::domain::snapshot::ConfigRevision::new(1),
            share::config::Config::default(),
        ),
    );
    let spec = parent_spec
        .derive_sub("coder", std::time::Duration::from_secs(30))
        .unwrap();
    let request = RunPreparationRequest::new(
        spec,
        session.snapshot_for_run(),
        Some(
            crate::application::run::preparation::ParentRunCapabilities::new(
                parent_run_id,
                parent_spec,
            ),
        ),
    )
    .unwrap();
    struct MissingParentResolver;
    impl crate::application::run::context_factory::RuntimeContextResolver for MissingParentResolver {
        fn resolve(
            &self,
            factory: &RuntimeContextFactory,
            request: &RunPreparationRequest,
        ) -> Result<
            (
                RuntimeContext,
                crate::application::run::preparation::SessionSnapshot,
            ),
            crate::application::run::preparation::RunPreparationError,
        > {
            factory
                .create(request.spec(), make_bindings(), None)
                .map(|context| (context, request.session().clone()))
                .map_err(|_| {
                    crate::application::run::preparation::RunPreparationError::ContextAssembly
                })
        }
    }
    let preparer = RunPreparer::new(context_factory, Arc::new(MissingParentResolver));

    assert!(matches!(
        preparer.prepare(request),
        Err(crate::application::run::preparation::RunPreparationError::ContextAssembly)
    ));
}

// ══════════════════════════════════════════════════════════════════

#[test]
fn assemble_parent_mediated_without_parent_fails() {
    // Sub spec has ParentMediated interaction.  Without a parent
    // RuntimeContext, assembly MUST fail with InteractionUnavailable.
    let f = factory();
    let spec = sub_spec(); // interaction_kind = ParentMediated
    let bindings = make_bindings();
    let result = f.create(&spec, bindings, None);
    assert_eq!(
        result.err(),
        Some(RuntimeContextAssemblyError::InteractionUnavailable)
    );
}

struct RecordingHook {
    points: Arc<std::sync::Mutex<Vec<hook::HookPoint>>>,
}

#[async_trait::async_trait]
impl HookPort for RecordingHook {
    async fn dispatch(
        &self,
        invocation: HookInvocation,
        _cancellation: &CancellationToken,
    ) -> HookOutcome {
        self.points
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(invocation.point());
        HookOutcome::proceed()
    }
}

#[tokio::test]
async fn boundary_only_hook_forwards_only_sub_run_boundaries() {
    let points = Arc::new(std::sync::Mutex::new(Vec::new()));
    let f = factory().with_hooks(Arc::new(RecordingHook {
        points: points.clone(),
    }));
    let parent_ctx = make_parent_context();
    let ctx = f
        .create(&sub_spec(), make_bindings(), Some(&parent_ctx))
        .unwrap();
    let cancellation = CancellationToken::new();

    ctx.hooks()
        .dispatch(
            HookInvocation::SubRunStart(hook::SubRunInput {
                prompt: "prompt".into(),
                system: "system".into(),
                model_spec: None,
            }),
            &cancellation,
        )
        .await;
    ctx.hooks()
        .dispatch(
            HookInvocation::PreToolUse(hook::PreToolUseInput {
                tool_name: "Bash".into(),
                tool_input: serde_json::json!({}),
            }),
            &cancellation,
        )
        .await;
    ctx.hooks()
        .dispatch(
            HookInvocation::SubRunStop(hook::SubRunStopInput {
                prompt: "prompt".into(),
                system: "system".into(),
                model_spec: None,
                result: "done".into(),
                turns: 1,
                is_error: false,
            }),
            &cancellation,
        )
        .await;

    assert_eq!(
        *points.lock().unwrap_or_else(|error| error.into_inner()),
        vec![hook::HookPoint::SubRunStart, hook::HookPoint::SubRunStop]
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
    let result = f.create(&spec, bindings, None);
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
    let result = f.create(&spec, bindings, None);
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
    let result = f.create(&spec, bindings, None);
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn assemble_main_ctx_has_client_interaction() {
    let f = factory();
    let spec = main_spec();
    let bindings = make_bindings();
    let ctx = f.create(&spec, bindings, None).unwrap();
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
    let result = f.create(&spec, bindings, Some(&parent_ctx));
    assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
}

#[test]
fn parent_mediated_interaction_is_child_scoped_and_not_parent_arc() {
    let f = factory();
    let parent_ctx = make_parent_context();
    let spec = sub_spec();
    let ctx = f.create(&spec, make_bindings(), Some(&parent_ctx)).unwrap();

    assert!(!Arc::ptr_eq(&ctx.interaction(), &parent_ctx.interaction()));

    let child_run = sdk::RunId::new_v7();
    let request_id = sdk::InteractionRequestId::new_v7();
    let request = sdk::InteractionRequest {
        id: request_id.clone(),
        run_id: child_run.clone(),
        body: sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
            reason: "pause".into(),
            recent_actions: vec![],
        }),
    };
    let receiver = ctx.interaction().register(request).unwrap();

    assert!(ctx.interaction().contains(&request_id));
    assert!(parent_ctx.interaction().contains(&request_id));
    assert_eq!(
        ctx.interaction()
            .reply(&request_id, sdk::InteractionReply::HardPauseContinue),
        sdk::InteractionCommandOutcome::Accepted
    );
    assert!(matches!(
        receiver.blocking_recv(),
        Ok(
            crate::application::interaction::port::InteractionCompletion::Replied(
                sdk::InteractionReply::HardPauseContinue
            )
        )
    ));

    let foreign_request = sdk::InteractionRequest {
        id: sdk::InteractionRequestId::new_v7(),
        run_id: sdk::RunId::new_v7(),
        body: sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
            reason: "foreign".into(),
            recent_actions: vec![],
        }),
    };
    assert!(matches!(
        ctx.interaction().register(foreign_request),
        Err(crate::application::interaction::port::InteractionPortError::WrongRun)
    ));
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
    bindings.model.reasoning = spy.clone();

    let ctx = f.create(&spec, bindings, None).unwrap();
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
    bindings.model.reasoning = spy.clone();

    let ctx = f.create(&spec, bindings, None).unwrap();
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
    let ctx = f.create(&spec, bindings, None).unwrap();

    // All service ports should be accessible (not panic).
    let _ = ctx.context();
    let _ = ctx.provider();
    let _ = ctx.tool_catalog();
    let _ = ctx.tool_execution();
    let _ = ctx.tool_context_binding();
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
    let ctx = f.create(&spec, bindings, None).unwrap();

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
    bindings.model.reasoning = spy.clone();

    let ctx = f.create(&spec, bindings, None).unwrap();
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
    let ctx = f.create(&spec, bindings, None).unwrap();

    let cancel = ctx.cancel();
    let child = cancel.child_scope();
    // verify no panic
    let _ = child.token();
}

#[test]
fn two_independent_assemblies_have_independent_cancel() {
    let f = factory();
    let spec = main_spec();
    let ctx1 = f.create(&spec, make_bindings(), None).unwrap();
    let ctx2 = f.create(&spec, make_bindings(), None).unwrap();

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

    let ctx1 = f.create(&spec, make_bindings(), None).unwrap();
    let ctx2 = f.create(&spec, make_bindings(), None).unwrap();

    // ── Cross-Run service ports: same Arc identity ──
    assert!(Arc::ptr_eq(&ctx1.tool_catalog(), &ctx2.tool_catalog()));
    assert!(Arc::ptr_eq(&ctx1.tool_execution(), &ctx2.tool_execution()));
    assert!(Arc::ptr_eq(
        &ctx1.tool_context_binding(),
        &ctx2.tool_context_binding()
    ));
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
    let parent_ctx = f.create(&main_spec(), make_bindings(), None).unwrap();
    let sub_ctx = f
        .create(&sub_spec(), make_bindings(), Some(&parent_ctx))
        .unwrap();
    let main_ctx = f.create(&main_spec(), make_bindings(), None).unwrap();

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
    }
    assert!(Arc::ptr_eq(&parent_ctx.hooks(), &main_ctx.hooks()));
    assert!(!Arc::ptr_eq(&sub_ctx.hooks(), &parent_ctx.hooks()));
}

// ══════════════════════════════════════════════════════════════════
// #1248 Task 3: Inherit reasoning — missing parent hard error
// ══════════════════════════════════════════════════════════════════

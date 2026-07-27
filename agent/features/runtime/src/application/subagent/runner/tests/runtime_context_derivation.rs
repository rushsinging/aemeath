use crate::application::interaction::InteractionBridge;
use crate::application::run_config::RunConfigSnapshot;
use crate::application::runtime_context::{
    RunCancellationScope, RunContextBindings, RunInputBufferHandle, RunUsageTracker, RuntimeContext,
};
use crate::application::runtime_context_factory::RuntimeContextFactory;
use crate::application::testing::test_task_access;
use crate::application::workspace_access::RuntimeWorkspaceAccess;
use crate::domain::agent_run::{RunKind, RunSpec};
use crate::ports::{
    ContextPort, PolicyDecision, PolicyPort, PolicyRequest, ProviderBinding, ProviderPort,
};
use std::sync::Arc;
use std::time::Duration;
use tools::{ToolCatalogPort, ToolProfileName};

// ── Minimal fake ports for standalone derivation tests ──

struct FakeCtxPort;
#[async_trait::async_trait]
impl ContextPort for FakeCtxPort {
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

struct FakeProvPort;
#[async_trait::async_trait]
impl ProviderPort for FakeProvPort {
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

pub(super) struct FakeToolCat;
impl ToolCatalogPort for FakeToolCat {
    fn snapshot(
        &self,
        scope: &tools::RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<tools::ToolCatalogSnapshot, tools::ToolCatalogError> {
        Ok(tools::ToolCatalogSnapshot::new(
            scope.clone(),
            profile.clone(),
            vec![],
        ))
    }
}

pub(super) struct FakeToolExec;
#[async_trait::async_trait]
impl tools::ToolExecutionPort for FakeToolExec {
    async fn execute(
        &self,
        _invocation: tools::ToolInvocation,
        _cancellation: &dyn tools::CancellationSignal,
    ) -> tools::ToolExecutionOutcome {
        tools::ToolExecutionOutcome::success_text("fake")
    }
}

pub(super) struct FakeToolCtxBind;
impl tools::ToolExecutionContextBindingPort for FakeToolCtxBind {
    fn bind(&self, _context: tools::ToolExecutionContext) -> Result<(), String> {
        Ok(())
    }
    fn unbind(&self, _run_id: &str) {}
}

pub(super) struct FakePolicyPort;
impl PolicyPort for FakePolicyPort {
    fn evaluate(&self, _request: &PolicyRequest) -> PolicyDecision {
        PolicyDecision::Allow(tools::AuthorizationContext::STANDARD)
    }
}

pub(super) struct FakeReflHist;
#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for FakeReflHist {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::MemoryError> {
        Ok(vec![])
    }
}
#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for FakeReflHist {
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

pub(super) struct FakeHookPort;
#[async_trait::async_trait]
impl hook::HookPort for FakeHookPort {
    async fn dispatch(
        &self,
        _invocation: hook::HookInvocation,
        _cancellation: &tokio_util::sync::CancellationToken,
    ) -> hook::HookOutcome {
        hook::HookOutcome::proceed()
    }
}
// ── Helper: build a parent RuntimeContext ──

/// Minimal no-op [`ChatEventSinkHandle`] for test assembly.
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

/// #1248 Task 3: shared factory-based helper for parent context construction.
fn assemble_parent_context(
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    config: RunConfigSnapshot,
) -> RuntimeContext {
    let provider_port: Arc<dyn ProviderPort> = Arc::new(FakeProvPort);
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
        tool_context_binding,
        Arc::new(FakePolicyPort),
        Arc::new(FakeReflHist),
        test_task_access(),
        Arc::new(FakeHookPort),
    );

    let bindings = RunContextBindings {
        context: Arc::new(FakeCtxPort),
        provider: Arc::new(binding),
        interaction: Arc::new(InteractionBridge::new()),
        memory: Arc::new(memory::NoOpMemory),
        config,
        cancel: RunCancellationScope::new(),
        event_sink: noop_event_sink(),
        usage: RunUsageTracker::new(),
        input: RunInputBufferHandle::new(),
        reasoning: Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium)),
        tool_catalog: None,
    };

    factory
        .assemble(&crate::domain::agent_run::RunSpec::main(), bindings, None)
        .expect("test parent context assembly")
}

pub(super) fn make_parent_context_with_catalog(
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
) -> RuntimeContext {
    let config_snapshot =
        crate::application::run_config::RunConfigSnapshot::capture(super::test_config_snapshot());
    assemble_parent_context(
        tool_catalog,
        tool_execution,
        tool_context_binding,
        config_snapshot,
    )
}

pub(super) fn make_parent_context_with_config(
    config_snapshot: share::config::domain::snapshot::ConfigSnapshot,
) -> RuntimeContext {
    let run_config = crate::application::run_config::RunConfigSnapshot::capture(config_snapshot);
    assemble_parent_context(
        Arc::new(FakeToolCat),
        Arc::new(FakeToolExec),
        Arc::new(FakeToolCtxBind),
        run_config,
    )
}

pub(super) fn make_parent_context() -> RuntimeContext {
    let mut config = share::config::Config::default();
    // coder → test-provider/test-model
    config.agents.roles.insert(
        "coder".to_string(),
        share::config::AgentRoleConfig {
            model: "test-provider/test-model".to_string(),
            ..Default::default()
        },
    );
    // role-a → role-a/model-a
    config.agents.roles.insert(
        "role-a".to_string(),
        share::config::AgentRoleConfig {
            model: "role-a/model-a".to_string(),
            ..Default::default()
        },
    );
    // role-b → role-b/model-b
    config.agents.roles.insert(
        "role-b".to_string(),
        share::config::AgentRoleConfig {
            model: "role-b/model-b".to_string(),
            ..Default::default()
        },
    );
    config.models.default = "test-provider/test-model".to_string();
    // test-provider → model test-model, driver openai
    config.models.providers.insert(
        "test-provider".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                api_style: None,
                context_window: 128000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    // role-a provider → model-a
    config.models.providers.insert(
        "role-a".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "model-a".to_string(),
                api_style: None,
                context_window: 128000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    // role-b provider → model-b
    config.models.providers.insert(
        "role-b".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "model-b".to_string(),
                api_style: None,
                context_window: 128000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    config.api.timeout = 30;

    let config_snapshot =
        RunConfigSnapshot::capture(share::config::domain::snapshot::ConfigSnapshot::new(config));
    assemble_parent_context(
        Arc::new(FakeToolCat),
        Arc::new(FakeToolExec),
        Arc::new(FakeToolCtxBind),
        config_snapshot,
    )
}

pub(super) fn make_parent_workspace() -> RuntimeWorkspaceAccess {
    let views = project::wire_production_workspace(std::env::temp_dir())
        .expect("wire test workspace")
        .into_views();
    RuntimeWorkspaceAccess::new(views)
}

/// #1248 Task 3: shared test factory for derive_sub_run calls.
fn make_test_factory() -> RuntimeContextFactory {
    RuntimeContextFactory::new(
        Arc::new(FakeToolCat),
        Arc::new(FakeToolExec),
        Arc::new(FakeToolCtxBind),
        Arc::new(FakePolicyPort),
        Arc::new(FakeReflHist),
        test_task_access(),
        Arc::new(FakeHookPort),
    )
}

/// #1248 Task 3: Build a parent RuntimeContext using the given factory
/// so that static ports (policy, hooks, etc.) are Arc-identical to
/// what derive_sub_run sees.
fn make_parent_context_with_factory(
    factory: &RuntimeContextFactory,
    config_snapshot: RunConfigSnapshot,
) -> RuntimeContext {
    let provider_port: Arc<dyn ProviderPort> = Arc::new(FakeProvPort);
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
    let bindings = RunContextBindings {
        context: Arc::new(FakeCtxPort),
        provider: Arc::new(binding),
        interaction: Arc::new(InteractionBridge::new()),
        memory: Arc::new(memory::NoOpMemory),
        config: config_snapshot,
        cancel: RunCancellationScope::new(),
        event_sink: noop_event_sink(),
        usage: RunUsageTracker::new(),
        input: RunInputBufferHandle::new(),
        reasoning: Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium)),
        tool_catalog: None,
    };
    factory
        .assemble(&RunSpec::main(), bindings, None)
        .expect("test parent context assembly")
}

// ── Test 1: cancellation ──

#[test]
fn sub_context_derivation_uses_parent_cancel_child_scope() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Child cancel token is derived from parent scope.
    let child_cancel = derived.context.cancel().clone();
    let parent_cancel = parent_ctx.cancel().clone();

    // Child cancel does NOT propagate to parent.
    child_cancel.token().cancel();
    assert!(child_cancel.token().is_cancelled());
    assert!(!parent_cancel.token().is_cancelled());

    // Parent cancel DOES propagate to a fresh child.
    let derived2 = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");
    let child2_cancel = derived2.context.cancel().clone();
    parent_cancel.token().cancel();
    assert!(child2_cancel.token().is_cancelled());
}

// ── Test 2: tools restricted ──

#[test]
fn sub_context_derivation_restricts_tool_catalog() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Derived tool catalog is not the parent's full catalog.
    assert!(!Arc::ptr_eq(
        &derived.context.tool_catalog(),
        &parent_ctx.tool_catalog()
    ));
    // The restricted snapshot must be retrievable.
    let snapshot = derived
        .context
        .tool_catalog()
        .snapshot(
            &tools::RegistryScopeName::new("sub-agent"),
            &ToolProfileName::new("sub-agent-restricted"),
        )
        .expect("restricted catalog snapshot must succeed");
    assert!(snapshot.tools.is_empty());
}

// ── Test 3: memory is NoOpMemory ──

#[test]
fn sub_context_derivation_disables_memory_by_default() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Sub memory is NOT the same Arc as parent.
    assert!(!Arc::ptr_eq(
        &derived.context.memory(),
        &parent_ctx.memory()
    ));
}

// ── Test 4: isolated context ──

#[test]
fn sub_context_derivation_uses_isolated_context() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Context port must be DIFFERENT from parent.
    assert!(!Arc::ptr_eq(
        &derived.context.context(),
        &parent_ctx.context()
    ));
}

// ── Test 5: policy same, interaction not parent's ──

#[test]
fn sub_context_derivation_does_not_widen_policy_or_interaction() {
    // #1248 Task 3: Share the factory between parent and sub so that
    // static ports (policy, etc.) are Arc-identical.
    let factory = Arc::new(make_test_factory());
    let parent_ctx = make_parent_context_with_factory(
        factory.as_ref(),
        RunConfigSnapshot::capture(super::test_config_snapshot()),
    );
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        factory,
    )
    .expect("derive_sub_run should succeed");

    // Policy: same Arc as parent (both from the same factory).
    assert!(Arc::ptr_eq(&derived.context.policy(), &parent_ctx.policy()));

    // Interaction: ParentMediated uses a child-scoped wrapper rather than
    // exposing the parent's port identity directly.
    assert!(!Arc::ptr_eq(
        &derived.context.interaction(),
        &parent_ctx.interaction()
    ));
}

// ── Test 6: derived spec is used by launcher ──

#[test]
fn sub_launcher_uses_derived_spec() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let parent_run_id = crate::domain::agent_run::RunId::new_v7();
    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        parent_run_id.clone(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    assert_eq!(derived.run.parent_id(), Some(&parent_run_id));
    // The derived spec must be a Sub-kind spec.
    assert_eq!(derived.run.spec().kind, RunKind::Sub);
    // The derived spec name contains the role.
    assert!(derived.run.spec().name.contains("coder"));
    // The derived spec must not be the same value as RunSpec::main().
    assert_ne!(derived.run.spec(), &RunSpec::main());
    // The derived spec timeout must match the request.
    assert_eq!(derived.run.spec().timeout, Duration::from_secs(30));
}

// ── Test 7: restricted catalog rejects non-sub-agent scope/profile ──

#[test]
fn sub_restricted_catalog_rejects_non_sub_agent_scope() {
    let parent_ctx = make_parent_context();
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let derived = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    let catalog = derived.context.tool_catalog();

    // sub-agent / sub-agent-restricted must succeed.
    catalog
        .snapshot(
            &tools::RegistryScopeName::new("sub-agent"),
            &ToolProfileName::new("sub-agent-restricted"),
        )
        .expect("sub-agent/sub-agent-restricted must succeed");

    // Any other scope/profile MUST be rejected.
    let bad_scopes = [
        ("main", "full"),
        ("general", "standard"),
        ("sub-agent", "full"),
        ("main", "sub-agent-restricted"),
        ("unknown", "unknown"),
    ];
    for &(scope, profile) in &bad_scopes {
        let result = catalog.snapshot(
            &tools::RegistryScopeName::new(scope),
            &ToolProfileName::new(profile),
        );
        assert!(
            result.is_err(),
            "restricted catalog must reject scope={scope} profile={profile}, got Ok"
        );
    }
}

// ── Test 8: derive_sub_run only queries parent catalog for sub-agent/sub-agent-restricted ──

/// A tool catalog that records every scope+profile pair queried via snapshot().
struct RecordingToolCatalog {
    /// Recorded (scope, profile) pairs in order.
    calls: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl ToolCatalogPort for RecordingToolCatalog {
    fn snapshot(
        &self,
        scope: &tools::RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<tools::ToolCatalogSnapshot, tools::ToolCatalogError> {
        self.calls
            .lock()
            .unwrap()
            .push((scope.to_string(), profile.to_string()));
        Ok(tools::ToolCatalogSnapshot::new(
            scope.clone(),
            profile.clone(),
            vec![],
        ))
    }
}

#[test]
fn sub_derivation_only_queries_sub_agent_scope_from_parent_catalog() {
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recording_catalog: Arc<dyn ToolCatalogPort> = Arc::new(RecordingToolCatalog {
        calls: calls.clone(),
    });

    let parent_ctx = make_parent_context_with_catalog(
        recording_catalog,
        Arc::new(FakeToolExec),
        Arc::new(FakeToolCtxBind),
    );
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    let recorded = calls.lock().unwrap();
    assert!(
        !recorded.is_empty(),
        "derive_sub_run must query the parent catalog at least once"
    );
    for (i, (scope, profile)) in recorded.iter().enumerate() {
        assert_eq!(
            scope.as_str(),
            "sub-agent",
            "call {i}: expected scope 'sub-agent', got '{scope}'"
        );
        assert_eq!(
            profile.as_str(),
            "sub-agent-restricted",
            "call {i}: expected profile 'sub-agent-restricted', got '{profile}'"
        );
    }
}

// ── Test 9: derive_sub_run fails closed when parent catalog returns error ──

/// A tool catalog that returns an error for sub-agent/sub-agent-restricted.
struct FailingToolCatalog;

impl ToolCatalogPort for FailingToolCatalog {
    fn snapshot(
        &self,
        scope: &tools::RegistryScopeName,
        profile: &ToolProfileName,
    ) -> Result<tools::ToolCatalogSnapshot, tools::ToolCatalogError> {
        if scope.as_str() == "sub-agent" && profile.as_str() == "sub-agent-restricted" {
            return Err(tools::ToolCatalogError::UnknownScope {
                scope: format!("{scope}/{profile} — injected failure"),
            });
        }
        Ok(tools::ToolCatalogSnapshot::new(
            scope.clone(),
            profile.clone(),
            vec![],
        ))
    }
}

#[test]
fn sub_derivation_fails_closed_when_parent_catalog_errors() {
    let failing_catalog: Arc<dyn ToolCatalogPort> = Arc::new(FailingToolCatalog);
    let parent_ctx = make_parent_context_with_catalog(
        failing_catalog,
        Arc::new(FakeToolExec),
        Arc::new(FakeToolCtxBind),
    );
    let parent_spec = RunSpec::main();
    let workspace = make_parent_workspace();
    let request = super::super::setup::SubRunRequest {
        role: "coder".to_string(),
        timeout: Duration::from_secs(30),
    };

    let result = super::super::setup::derive_sub_run(
        &parent_spec,
        &parent_ctx,
        &workspace,
        crate::domain::agent_run::RunId::new_v7(),
        &request,
        &crate::ports::provider_port::fake::FakeProviderFactory,
        super::empty_skill_materializer(),
        Arc::new(make_test_factory()),
    );

    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("injected failure") || msg.contains("UnknownScope"),
                "error must propagate the catalog error, got: {msg}"
            );
        }
        Ok(_) => panic!("derive_sub_run must fail when parent catalog returns error"),
    }
}

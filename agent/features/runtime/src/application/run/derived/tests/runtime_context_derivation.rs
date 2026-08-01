use crate::application::run::config::RunConfigSnapshot;
use crate::application::run::context::RuntimeContext;
use crate::application::run::context_factory::RuntimeContextFactory;
use crate::application::run::run_factory_support::SessionRunFixture;
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::domain::agent_run::RunSpec;
use crate::ports::{PolicyDecision, PolicyPort, PolicyRequest};
use std::sync::Arc;
use std::time::Duration;
use tools::{ToolCatalogPort, ToolProfileName};

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
        _context: &tools::ToolExecutionContext,
    ) -> tools::ToolExecutionOutcome {
        tools::ToolExecutionOutcome::success_text("fake")
    }
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
        _cancellation: &dyn hook::CancellationSignal,
    ) -> hook::HookOutcome {
        hook::HookOutcome::proceed()
    }
}
// ── Helper: build a parent RuntimeContext through RunFactory ──

fn assemble_parent_context(
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    config: RunConfigSnapshot,
) -> RuntimeContext {
    SessionRunFixture::builder()
        .with_tool_catalog(tool_catalog)
        .with_tool_execution(tool_execution)
        .with_config(config.config().clone())
        .build()
        .create(RunSpec::main())
        .expect("test parent run creation")
        .context()
        .clone()
}

#[derive(Clone, Default)]
struct RecordingEventSink {
    events: Arc<std::sync::Mutex<Vec<crate::application::loop_engine::chat::RuntimeStreamEvent>>>,
}

impl crate::application::loop_engine::chat::ChatEventSink for RecordingEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::application::loop_engine::chat::RuntimeStreamEvent,
    ) -> crate::application::loop_engine::chat::EventFuture<'a> {
        self.events.lock().unwrap().push(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send_event(&self, event: crate::application::loop_engine::chat::RuntimeStreamEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn make_parent_context_with_event_sink(
    event_sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
) -> RuntimeContext {
    let mut config = share::config::Config::default();
    config.agents.roles.insert(
        "coder".to_string(),
        share::config::AgentRoleConfig {
            model: "test-provider/test-model".to_string(),
            ..Default::default()
        },
    );
    config.models.default = "test-provider/test-model".to_string();
    config.models.providers.insert(
        "test-provider".to_string(),
        share::config::models::ProviderModelsConfig {
            driver: "openai".to_string(),
            models: vec![share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                context_window: 128_000,
                max_tokens: 8192,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    SessionRunFixture::builder()
        .with_config(share::config::domain::snapshot::ConfigSnapshot::new(config))
        .with_event_sink_handle(event_sink)
        .build()
        .create(RunSpec::main())
        .expect("test parent run creation")
        .context()
        .clone()
}

#[tokio::test]
async fn derived_context_does_not_publish_raw_child_events_to_parent_sink() {
    use crate::application::loop_engine::chat::ChatEventSink as _;

    let recording = RecordingEventSink::default();
    let recorded_events = recording.events.clone();
    let parent_context = make_parent_context_with_event_sink(
        crate::application::loop_engine::chat::ChatEventSinkHandle::new(recording),
    );
    let derived = super::super::setup::derive_sub_run(
        &RunSpec::main(),
        &parent_context,
        &make_parent_workspace(),
        crate::domain::agent_run::RunId::new_v7(),
        &super::super::setup::SubRunRequest {
            role: "coder".to_string(),
            timeout: Duration::from_secs(30),
        },
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    derived
        .instance
        .context()
        .event_sink()
        .send_event(
            crate::application::loop_engine::chat::RuntimeStreamEvent::SystemMessage(
                "child-raw-event".to_string(),
            ),
        )
        .await;

    assert!(
        recorded_events.lock().unwrap().is_empty(),
        "Derived Run 的原始事件不得直接进入父 Main UI sink"
    );
}

pub(super) fn make_parent_context_with_catalog(
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
) -> RuntimeContext {
    let config_snapshot =
        crate::application::run::config::RunConfigSnapshot::capture(super::test_config_snapshot());
    assemble_parent_context(tool_catalog, tool_execution, config_snapshot)
}

pub(super) fn make_parent_context_with_config(
    config_snapshot: share::config::domain::snapshot::ConfigSnapshot,
) -> RuntimeContext {
    let run_config = crate::application::run::config::RunConfigSnapshot::capture(config_snapshot);
    assemble_parent_context(Arc::new(FakeToolCat), Arc::new(FakeToolExec), run_config)
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
        config_snapshot,
    )
}

pub(super) fn make_parent_workspace() -> RuntimeWorkspaceAccess {
    let views = project::wire_production_workspace(std::env::temp_dir())
        .expect("wire test workspace")
        .into_views();
    RuntimeWorkspaceAccess::new(views)
}

/// Shared test factory for Derived Run creation.
fn make_test_factory() -> RuntimeContextFactory {
    RuntimeContextFactory::new(
        Arc::new(FakeToolCat),
        Arc::new(FakeToolExec),
        Arc::new(FakePolicyPort),
        Arc::new(FakeReflHist),
        crate::application::run::test_task_access(),
        Arc::new(FakeHookPort),
    )
}

/// Build a parent RuntimeContext using the given factory so static ports
/// remain Arc-identical to the services seen by Derived Run creation.
fn make_parent_context_with_factory(
    factory: &Arc<RuntimeContextFactory>,
    config_snapshot: RunConfigSnapshot,
) -> RuntimeContext {
    SessionRunFixture::builder()
        .with_context_factory(factory.clone())
        .with_config(config_snapshot.config().clone())
        .build()
        .create(RunSpec::main())
        .expect("test parent run creation")
        .context()
        .clone()
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Child cancel token is derived from parent scope.
    let child_cancel = derived.instance.context().cancel().clone();
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");
    let child2_cancel = derived2.instance.context().cancel().clone();
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Derived tool catalog is not the parent's full catalog.
    assert!(!Arc::ptr_eq(
        &derived.instance.context().tool_catalog(),
        &parent_ctx.tool_catalog()
    ));
    // The restricted snapshot must be retrievable.
    let snapshot = derived
        .instance
        .context()
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Sub memory is NOT the same Arc as parent.
    assert!(!Arc::ptr_eq(
        &derived.instance.context().memory(),
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    // Context port must be DIFFERENT from parent.
    assert!(!Arc::ptr_eq(
        &derived.instance.context().context(),
        &parent_ctx.context()
    ));
}

// ── Test 5: policy same, interaction not parent's ──

#[test]
fn sub_context_derivation_does_not_widen_policy_or_interaction() {
    // Parent 与 Derived 共用基础 factory，保证 policy 等静态服务保持
    // Arc 身份一致。
    let factory = Arc::new(make_test_factory());
    let parent_ctx = make_parent_context_with_factory(
        &factory,
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        factory,
    )
    .expect("derive_sub_run should succeed");

    // Policy: same Arc as parent (both from the same factory).
    assert!(Arc::ptr_eq(
        &derived.instance.context().policy(),
        &parent_ctx.policy()
    ));

    // Interaction: ParentMediated uses a child-scoped wrapper rather than
    // exposing the parent's port identity directly.
    assert!(!Arc::ptr_eq(
        &derived.instance.context().interaction(),
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    assert_eq!(derived.instance.run().parent_id(), Some(&parent_run_id));
    // The derived spec carries restricted capabilities rather than a role tag.
    assert_eq!(
        derived.instance.run().spec().input,
        crate::domain::agent_run::InputMode::Fixed
    );
    assert_eq!(
        derived.instance.run().spec().tools,
        crate::domain::agent_run::ToolScope::Restricted
    );
    // The derived spec name contains the role.
    assert!(derived.instance.run().spec().name.contains("coder"));
    // The derived spec must not be the same value as RunSpec::main().
    assert_ne!(derived.instance.run().spec(), &RunSpec::main());
    // The derived spec timeout must match the request.
    assert_eq!(
        derived.instance.run().spec().timeout,
        Duration::from_secs(30)
    );
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
        Arc::new(make_test_factory()),
    )
    .expect("derive_sub_run should succeed");

    let catalog = derived.instance.context().tool_catalog();

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

    let parent_ctx = make_parent_context_with_catalog(recording_catalog, Arc::new(FakeToolExec));
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
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
    let parent_ctx = make_parent_context_with_catalog(failing_catalog, Arc::new(FakeToolExec));
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
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        tools::composition::wire_skills().catalog(),
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

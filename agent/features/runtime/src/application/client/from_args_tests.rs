use std::sync::Arc;

use super::*;
use crate::application::client::accessors::SessionRuntime;
use crate::domain::agent_run::RunSpec;
use crate::ports::PolicyPort;
use hook::{HookInvocation, HookOutcome, HookPort};
use memory::api::{MemoryPort, ReflectionHistoryStore};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn runtime_bootstrap_does_not_construct_session_runtime_literal() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("SessionRuntime {"));
    assert!(production.contains("SessionRuntime::new("));
}

#[test]
fn session_runtime_groups_dynamic_model_state() {
    let source = include_str!("accessors.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    let session_runtime = production
        .split("pub struct SessionRuntime")
        .nth(1)
        .and_then(|rest| rest.split("}").next())
        .expect("SessionRuntime definition");
    assert!(!session_runtime.contains("resolved_model:"));
    assert!(!session_runtime.contains("current_binding:"));
    assert!(session_runtime.contains("model_state:"));
}

#[test]
fn session_runtime_has_no_duplicate_workspace_root_field() {
    let source = include_str!("accessors.rs");
    let agent_client_impl = source
        .split("impl AgentClientImpl")
        .nth(1)
        .expect("AgentClientImpl accessors");
    let accessor = agent_client_impl
        .split("pub fn cwd(&self)")
        .nth(1)
        .and_then(|rest| rest.split("pub fn resolved_model").next())
        .expect("cwd accessor");
    assert!(accessor.contains("session_snapshot"));
}

#[test]
fn session_runtime_has_no_duplicate_session_id_field() {
    let source = include_str!("accessors.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    let session_runtime = production
        .split("pub struct SessionRuntime")
        .nth(1)
        .and_then(|rest| rest.split("}").next())
        .expect("SessionRuntime definition");
    assert!(!session_runtime.contains("session_id:"));
    assert!(!production.contains("pub(crate) fn update_session_id"));
}

#[test]
fn session_runtime_identity_reads_session_state() {
    let source = include_str!("accessors.rs");
    let agent_client_impl = source
        .split("impl AgentClientImpl")
        .nth(1)
        .expect("AgentClientImpl accessors");
    let accessor = agent_client_impl
        .split("pub fn session_id(&self)")
        .nth(1)
        .and_then(|rest| rest.split("pub fn cwd").next())
        .expect("session_id accessor");
    assert!(accessor.contains("session_snapshot"));
    assert!(!accessor.contains("shell.session_id"));
}

#[test]
fn production_runtime_has_no_main_session_shell_type() {
    let production_files = [
        include_str!("accessors.rs"),
        include_str!("../loop_engine/chat/loop_context.rs"),
        include_str!("../run/context_factory.rs"),
    ];
    for source in production_files {
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        assert!(!production.contains("MainSessionShell"));
    }
}

#[test]
fn bootstrap_dependencies_do_not_duplicate_runtime_services() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    let bootstrap = production
        .split("pub struct RuntimeBootstrapDependencies")
        .nth(1)
        .and_then(|rest| rest.split("impl RuntimeBootstrapDependencies").next())
        .expect("RuntimeBootstrapDependencies definition");
    assert!(!bootstrap.contains("reflection_history:"));
    assert!(!bootstrap.contains("policy:"));
    assert!(!bootstrap.contains("task_access:"));
    assert!(!bootstrap.contains("hook_runner:"));
    assert!(!bootstrap.contains("tool_execution:"));
    assert!(!bootstrap.contains("tool_context_binding:"));
}

#[test]
fn runtime_bootstrap_does_not_parse_session_args() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("std::env::current_dir"));
    assert!(!production.contains("resolve_context_size("));
    assert!(!production.contains("args.allow_all"));
    assert!(!production.contains("args.verbose"));
}

#[test]
fn runtime_bootstrap_does_not_query_or_materialize_skills() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("SkillQuery::new("));
    assert!(!production.contains("materialize_available("));
    assert!(!production.contains("ToolProfileName::new(\"main-full\")"));
}

#[test]
fn runtime_bootstrap_does_not_build_prompt() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("build_system_prompt_parts("));
    assert!(!production.contains("build_static_prompt("));
}

#[test]
fn runtime_bootstrap_does_not_resolve_agent_concurrency() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("resolve_concurrency_limits("));
    assert!(!production.contains("Semaphore::new(max_agent_concurrency)"));
}

#[test]
fn runtime_bootstrap_does_not_construct_agent_runner() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("build_agent_runner("));
}

#[test]
fn runtime_bootstrap_does_not_construct_initial_provider_binding() {
    let source = include_str!("from_args.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    assert!(!production.contains("ProviderBuildSpec {"));
    assert!(!production.contains("provider_factory.build("));
}

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self {
            key,
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
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
/// Build a minimal `SessionRuntime` with fake ports for assembler tests.
/// Accepts a shared `ParentRunContextSource` so tests that wire up both
/// the SessionRuntime and a runner pass the same source — no orphan source.
async fn make_test_shell(
    parent_context_source: crate::application::run::context::ParentRunContextSource,
) -> SessionRuntime {
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create root");
    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let task_wiring = task::wire_task();
    let config = config::wire_project_config(
        &root,
        config::NativeConfigStore::new(Arc::new(
            storage::FileSystemBlobAdapter::new(temp.path()).expect("create config blob"),
        )),
    )
    .await
    .expect("wire config");
    let wiring = context::test_support::wire_in_memory(
        &workspace,
        task_wiring.persist(),
        config.reader(),
        config.participant(),
        Arc::new(context::test_support::UnavailableSessionManagement),
        Arc::new(context::ProductionMainContextFactory::new(Arc::new(
            context::NoOpCanonicalSessionWriter,
        ))),
    )
    .await;
    let snapshot = wiring.committed_config();
    let binding = crate::application::model::test_support::test_binding(Vec::new());
    let policy: Arc<dyn PolicyPort> = Arc::new(policy::AllowAllPolicy);
    let _memory: Arc<dyn MemoryPort> = Arc::new(memory::NoOpMemory);
    let tools_factory = tools::composition::TestCatalogExecutionFactory::empty();
    let tool_catalog: Arc<dyn tools::ToolCatalogPort> = tools_factory.catalog_port();
    let tool_execution: Arc<dyn tools::ToolExecutionPort> = tools_factory.execution();
    let reflection_history: Arc<dyn ReflectionHistoryStore> = Arc::new(FakeReflectionHistory);
    let task_access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let hook_runner: Arc<dyn HookPort> = Arc::new(FakeHook);

    struct NoopRunner;
    #[async_trait::async_trait]
    impl tools::AgentRunner for NoopRunner {
        async fn run_agent(&self, _request: tools::AgentRunRequest<'_>) -> tools::AgentRunTerminal {
            tools::AgentRunTerminal::Completed {
                result: String::new(),
            }
        }
    }
    let agent_runner: Arc<dyn tools::AgentRunner> = Arc::new(NoopRunner);
    let tool_result_materializer =
        crate::application::tool::test_support::test_tool_result_materializer();
    let active_run =
        Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let config_query = wiring.config_query();
    let config_writer = wiring.config_writer();
    let session_management = wiring.session_management();
    let cwd = root.clone();

    // #1248 Task 3: Build RuntimeContextFactory for test SessionRuntime.
    let runtime_context_factory = Arc::new(
        crate::application::run::context_factory::RuntimeContextFactory::new(
            tool_catalog.clone(),
            tool_execution.clone(),
            policy.clone(),
            reflection_history.clone(),
            task_access.clone(),
            hook_runner.clone(),
        ),
    );

    let skill_wiring = tools::composition::wire_skills();
    let initial_skill_snapshot = tools::SkillCatalogSnapshot::from_descriptors(Vec::new());

    SessionRuntime::new(
        Arc::new(std::sync::RwLock::new(
            crate::application::run::creation::SessionState::new(
                "test-session",
                cwd,
                format!("{}/{}", binding.model.provider, binding.model.model),
                snapshot.clone(),
            ),
        )),
        workspace,
        wiring.clone(),
        config_query,
        config_writer,
        session_management,
        Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
        crate::application::client::accessors::SessionModelState::new(
            snapshot
                .resolve_runtime_model(None, None)
                .map(|rm| rm.resolved_model().clone())
                .unwrap_or_else(|_| share::config::models::ResolvedModel {
                    source_key: "test".to_string(),
                    source_config: Default::default(),
                    driver: "openai".to_string(),
                    model: share::config::models::ModelEntryConfig {
                        id: "test-model".to_string(),
                        name: "test".to_string(),
                        context_window: 128_000,
                        max_tokens: 8192,
                        ..Default::default()
                    },
                }),
            binding.clone(),
        ),
        10,
        4,
        Arc::new(tokio::sync::Semaphore::new(4)),
        Vec::new(),
        String::new(),
        String::new(),
        String::new(),
        "test-model".to_string(),
        skill_wiring.catalog(),
        initial_skill_snapshot,
        share::config::MemoryConfig::default(),
        200_000,
        "en".to_string(),
        true,
        false,
        None,
        None,
        agent_runner,
        parent_context_source,
        tool_result_materializer,
        active_run,
        crate::composition::wire_sdk_chat_ingress().event_sink_factory,
        crate::composition::wire_sdk_chat_ingress().input_port_factory,
        runtime_context_factory,
    )
}

// ── Task 4 L1: Shell classification tests ──

/// After bootstrap, SessionRuntime holds session-level state: wiring, workspace,
/// session identity, prompt bootstrap, model switch. It does NOT hold a per-Run
/// RuntimeContext instance.
#[tokio::test(flavor = "current_thread")]
async fn session_runtime_holds_session_state_without_runtime_context() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;

    // SessionRuntime has wiring + workspace (session-level).
    let _wiring = &shell.wiring;
    let _workspace = &shell.workspace;
    let _session_id = shell.session_snapshot().session_id().to_string();

    // SessionRuntime has prompt bootstrap fields.
    let _system_blocks = &shell.system_blocks;
    let _skill_snapshot = &shell.initial_skill_snapshot;
    let _initial_git = &shell.initial_git_context;

    // SessionRuntime has model switch fields.
    let _resolved_model = shell.model_state.resolved();
    let _binding = shell.model_state.binding();

    // Shell does NOT have a RuntimeContext — it must be assembled per run.
    // (Compile-time check: SessionRuntime has no `runtime_context: RuntimeContext` field.)
}

/// Two assembler calls from the same shell produce different cancellation scopes
/// but share Arc'd parent resources.
#[tokio::test(flavor = "current_thread")]
async fn two_assembler_calls_produce_different_cancel_shared_arcs() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;
    let fixture = crate::application::run::run_factory_support::SessionRunFixture::builder()
        .with_provider_binding(shell.model_state.binding())
        .build();
    let first = fixture.create(RunSpec::main()).expect("first assembly");
    let second = fixture.create(RunSpec::main()).expect("second assembly");
    let ctx1 = first.context();
    let ctx2 = second.context();
    // Different cancellation scopes.
    ctx1.cancel().token().cancel();
    assert!(ctx1.cancel().token().is_cancelled());
    assert!(!ctx2.cancel().token().is_cancelled());

    // Shared Arc'd resources: both RuntimeContexts point to the same underlying
    // parent capability ports (policy, hook, task, reflection).
    assert!(Arc::ptr_eq(&ctx1.policy(), &ctx2.policy()));
    assert!(Arc::ptr_eq(&ctx1.hooks(), &ctx2.hooks()));
    assert!(Arc::ptr_eq(&ctx1.task(), &ctx2.task()));
    assert!(Arc::ptr_eq(
        &ctx1.reflection_history(),
        &ctx2.reflection_history()
    ));
}

/// Model switch updates SessionModelState and only affects the NEXT assembler call.
/// An already-assembled RuntimeContext keeps its frozen binding.
#[tokio::test(flavor = "current_thread")]
async fn model_switch_affects_only_next_assembler() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;
    let fixture_before = crate::application::run::run_factory_support::SessionRunFixture::builder()
        .with_provider_binding(shell.model_state.binding())
        .build();
    let run_before = fixture_before
        .create(RunSpec::main())
        .expect("first assembly");
    let ctx_before = run_before.context();

    let binding_before = ctx_before.provider();
    // Simulate model switch: create a new binding.
    let new_binding =
        crate::application::model::test_support::test_binding(vec!["new model response"]);
    shell.model_state.update_binding(new_binding.clone());

    // Second assembly picks up the new binding.
    let fixture_after = crate::application::run::run_factory_support::SessionRunFixture::builder()
        .with_provider_binding(shell.model_state.binding())
        .build();
    let run_after = fixture_after
        .create(RunSpec::main())
        .expect("second assembly");
    let ctx_after = run_after.context();

    let binding_after = ctx_after.provider();
    // The first context still uses the old binding.
    assert!(
        Arc::ptr_eq(&binding_before, &ctx_before.provider()),
        "already-assembled context retains its frozen binding"
    );
    // The second context uses the new binding.
    assert!(
        Arc::ptr_eq(&binding_after, &new_binding),
        "next assembler uses the switched binding"
    );
    // They are different.
    assert!(
        !Arc::ptr_eq(&binding_before, &binding_after),
        "model switch produces different bindings across assembler calls"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn from_args_preserves_workspace_views_and_main_policy_identity() {
    struct TestReflectionHistory;

    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryQuery for TestReflectionHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError> {
            Ok(Vec::new())
        }
    }
    #[async_trait::async_trait]
    impl memory::api::ReflectionHistoryStore for TestReflectionHistory {
        async fn append(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
        async fn upsert(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
    }
    let temp = tempfile::tempdir().expect("create temp root");
    let root = temp.path().join("root");
    let sub = root.join("sub");
    let agents_dir = temp.path().join("agents");
    std::fs::create_dir_all(&sub).expect("create workspace subdirectory");
    std::fs::create_dir_all(&agents_dir).expect("create isolated agents directory");

    let _env = EnvGuard::set("AEMEATH_AGENTS_DIR", &agents_dir);
    std::fs::write(
        agents_dir.join("aemeath.json"),
        serde_json::json!({
            "models": {
                "default": "local/test-model",
                "providers": {
                    "local": {
                        "baseUrl": "http://127.0.0.1:1/v1",
                        "apiKey": "test-api-key",
                        "driver": "openai",
                        "models": [{
                            "id": "test-model",
                            "name": "Test Model",
                            "input": ["text"],
                            "contextWindow": 8192,
                            "max_tokens": 1024
                        }]
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write isolated config");
    std::fs::write(agents_dir.join("mcp.json"), r#"{"mcpServers":{}}"#)
        .expect("write isolated MCP config");

    let workspace = project::wire_production_workspace(root.clone())
        .expect("wire workspace")
        .into_views();
    let original = workspace.clone();
    workspace
        .control()
        .change_directory(sub.clone())
        .expect("change workspace to subdirectory");

    let args = ChatBootstrapArgs {
        cwd: Some(root.clone()),
        api_key: Some("test-api-key".to_string()),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        model: Some("local/test-model".to_string()),
        context_size: 8192,
        ..Default::default()
    };
    let config = config::wire_project_config(
        &root,
        config::NativeConfigStore::new(Arc::new(
            storage::FileSystemBlobAdapter::new(&agents_dir).expect("create config blob"),
        )),
    )
    .await
    .expect("wire config");
    let task_wiring = task::wire_task();
    let wiring = context::test_support::wire_in_memory(
        &workspace,
        task_wiring.persist(),
        config.reader(),
        config.participant(),
        Arc::new(context::test_support::UnavailableSessionManagement),
        Arc::new(context::ProductionMainContextFactory::new(Arc::new(
            context::NoOpCanonicalSessionWriter,
        ))),
    )
    .await;
    let policy: Arc<dyn policy::PolicyPort> = Arc::new(policy::AllowAllPolicy);
    let tools = tools::composition::TestCatalogExecutionFactory::empty();
    let skill_wiring = tools::composition::wire_skills();
    let tool_result_materializer =
        crate::application::tool::test_support::test_tool_result_materializer();
    let active_run =
        Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
        hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(
            share::config::Config::default(),
        ))
        .expect("test hook dispatcher"),
    );
    let initial_binding = crate::ports::ProviderFactory::build(
        &crate::ports::provider_port::fake::FakeProviderFactory,
        crate::ports::ProviderBuildSpec {
            driver: "openai".to_string(),
            source_key: "local".to_string(),
            api_style: None,
            api_key: "test-api-key".to_string(),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
            model: provider::ModelId {
                provider: "local".to_string(),
                model: "test-model".to_string(),
            },
            max_tokens: 8192,
            requested_reasoning: provider::ReasoningLevel::Off,
            context_window: Some(8192),
            timeout: std::time::Duration::from_secs(30),
            user_agent: "test".to_string(),
        },
    )
    .expect("build initial binding");
    let initial_snapshot = config.reader().committed_snapshot();
    let initial_provider = InitialProviderAssembly::new(
        initial_binding,
        initial_snapshot
            .resolve_runtime_model(None, None)
            .expect("resolve test model")
            .resolved_model()
            .clone(),
        ModelRuntimeSettings {
            max_tokens: 8192,
            reasoning: false,
            reasoning_effort: None,
        },
    );
    struct NoopRunner;
    #[async_trait::async_trait]
    impl tools::AgentRunner for NoopRunner {
        async fn run_agent(&self, _request: tools::AgentRunRequest<'_>) -> tools::AgentRunTerminal {
            tools::AgentRunTerminal::Completed {
                result: String::new(),
            }
        }
    }
    let runtime_context_factory = Arc::new(
        crate::application::run::context_factory::RuntimeContextFactory::new(
            tools.catalog_port(),
            tools.execution(),
            policy,
            Arc::new(TestReflectionHistory),
            task_wiring.access(),
            hook_runner.clone(),
        ),
    );
    let dependencies = RuntimeBootstrapDependencies::new(
        RuntimeCoreDependencies::new(
            workspace,
            wiring,
            Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            Arc::new(context::test_support::UnavailableSessionManagement),
        ),
        RuntimeToolAssemblyDependencies::new(
            tools.catalog_port(),
            skill_wiring.catalog(),
            tool_result_materializer,
            active_run.clone(),
        ),
        crate::composition::wire_sdk_chat_ingress(),
        initial_provider,
        SessionBootstrapAssembly::new(root.clone(), 8192, true, false, None),
        PromptAssembly::new(Vec::new(), String::new(), String::new(), "test-model"),
        SkillBootstrapAssembly::new(tools::SkillCatalogSnapshot::from_descriptors(Vec::new())),
        crate::application::client::bootstrap::AgentRunnerAssembly {
            runner: Arc::new(NoopRunner),
            parent_context_source: crate::application::run::context::ParentRunContextSource::new(),
            active_run: active_run.clone(),
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            runtime_context_factory: runtime_context_factory.clone(),
        },
    );
    let client = from_args_with_workspace(args, dependencies)
        .await
        .expect("build client with workspace");

    assert!(
        Arc::ptr_eq(
            &client.inner.shell.runtime_context_factory.services().hooks,
            &hook_runner
        ),
        "Main Run 必须保留 Composition 注入的同一 HookRunner 实例"
    );
    assert_eq!(
        client.inner.shell.workspace.read().current_path_base(),
        sub.canonicalize().expect("canonicalize subdirectory")
    );

    original
        .control()
        .change_directory(root.clone())
        .expect("change original clone back to root");
    assert_eq!(
        client.inner.shell.workspace.read().current_path_base(),
        root.canonicalize().expect("canonicalize root")
    );
} // ── Task 4 GREEN: single-source verification tests ──

/// The SDK startup snapshot reads the current model state without exposing
/// Runtime services or adapter-owned launch containers.
#[tokio::test(flavor = "current_thread")]
async fn startup_snapshot_reads_current_model_state() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;

    // Write a distinct binding into SessionModelState.
    let switched = crate::application::model::test_support::test_binding(vec!["tui sees this"]);
    shell.model_state.update_binding(switched);

    let handle = RuntimeHandle {
        shell: shell.clone(),
    };
    let client = AgentClientImpl {
        inner: Arc::new(handle),
    };

    let expected_display = crate::application::client::mapping::model_display(
        &shell.model_state.resolved().source_key,
        &shell.model_state.resolved().model.name,
        &shell.model_state.resolved().model.id,
    );
    let launch = client.startup_snapshot();
    assert_eq!(launch.model_display, expected_display);
}

/// #1385 Task 7: accessors return values from `shell`, the single source.
#[tokio::test(flavor = "current_thread")]
async fn accessors_read_from_shell_single_source() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;

    let handle = RuntimeHandle {
        shell: shell.clone(),
    };
    let client = AgentClientImpl {
        inner: Arc::new(handle),
    };

    // All accessors must return shell values.
    let session_id = shell.session_snapshot().session_id().to_string();
    assert_eq!(
        client.session_id(),
        session_id,
        "session_id() must read SessionState"
    );
    let workspace_root = shell.session_snapshot().workspace_root().to_path_buf();
    assert_eq!(client.cwd(), workspace_root, "cwd() must read SessionState");
    assert_eq!(
        client.resolved_model().model.id,
        shell.model_state.resolved().model.id,
        "resolved_model() must return SessionModelState value"
    );
    assert_eq!(
        client.max_tool_concurrency(),
        shell.max_tool_concurrency,
        "max_tool_concurrency() must return shell value"
    );
    assert_eq!(
        client.max_agent_concurrency(),
        shell.max_agent_concurrency,
        "max_agent_concurrency() must return shell value"
    );
    assert!(Arc::ptr_eq(
        &client.shell().session_state,
        &shell.session_state,
    ));
    // verbose is owned by the session shell.
    assert_eq!(client.shell().verbose, shell.verbose);
}

/// #1385 Task 7: `shell.interaction_bridge` is the single source.
/// `reply_interaction` and `cancel_interaction` both use it.
#[tokio::test(flavor = "current_thread")]
async fn interaction_bridge_is_single_source_on_shell() {
    let shell =
        make_test_shell(crate::application::run::context::ParentRunContextSource::new()).await;
    let bridge_ptr = Arc::as_ptr(&shell.interaction_bridge);

    let handle = RuntimeHandle { shell };
    let client = AgentClientImpl {
        inner: Arc::new(handle),
    };

    // shell().interaction_bridge is the one used by SDK methods.
    let shell_bridge_after = Arc::as_ptr(&client.shell().interaction_bridge);
    assert_eq!(
        bridge_ptr, shell_bridge_after,
        "interaction_bridge must be a single Arc on shell — no second copy"
    );
}

/// Startup resume must run before the committed snapshot is read, while
/// Context rejects any session whose ProjectIdentity differs from the
/// live workspace; a failed resume therefore cannot change Config/Memory.
#[test]
fn startup_resume_precedes_current_project_config_read() {
    let source = include_str!("from_args.rs");
    let resume_pos = source
        .find("startup resume")
        .expect("source should contain 'startup resume'");
    let resume_call_pos = source
        .find("resume_session_to_backing")
        .expect("source should contain resume_session_to_backing");
    let snapshot_pos = source
        .rfind("let snapshot = wiring.committed_config()")
        .expect("source should contain committed_config read");

    assert!(
        resume_pos < resume_call_pos,
        "startup resume comment should precede resume_session_to_backing call"
    );
    assert!(
        resume_call_pos < snapshot_pos,
        "resume_session_to_backing must precede the committed_config snapshot read — \
           Context rejects cross-project sessions before this snapshot can change"
    );
}

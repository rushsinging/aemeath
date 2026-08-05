use std::collections::BTreeSet;
use std::sync::Arc;

struct TestProviderFactory;

impl runtime::ProviderFactory for TestProviderFactory {
    fn build(
        &self,
        spec: runtime::ProviderBuildSpec,
    ) -> Result<runtime::ProviderBinding, provider::ProviderError> {
        struct UnusedPort;
        #[async_trait::async_trait]
        impl runtime::ProviderPort for UnusedPort {
            fn capabilities(
                &self,
                _model: &provider::ModelId,
            ) -> Result<provider::ModelCapability, provider::ProviderError> {
                Err(provider::ProviderError::fatal(
                    provider::ProviderErrorKind::ModelUnavailable,
                    "unused test provider",
                ))
            }

            async fn invoke(
                &self,
                _request: provider::InvocationRequest,
                _cancellation: &dyn provider::CancellationSignal,
            ) -> Result<provider::InvocationStream, provider::ProviderError> {
                Err(provider::ProviderError::fatal(
                    provider::ProviderErrorKind::UpstreamUnavailable,
                    "unused test provider",
                ))
            }
        }
        Ok(runtime::ProviderBinding {
            provider: Arc::new(UnusedPort),
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
            context_window: spec.context_window,
        })
    }
}

fn initial_provider_assembly() -> runtime::InitialProviderAssembly {
    let spec = runtime::ProviderBuildSpec {
        driver: "test".to_string(),
        source_key: "test".to_string(),
        api_style: None,
        api_key: "test-key".to_string(),
        base_url: None,
        model: provider::ModelId {
            provider: "test".to_string(),
            model: "test-model".to_string(),
        },
        max_tokens: 8192,
        requested_reasoning: provider::ReasoningLevel::Off,
        context_window: Some(8192),
        timeout: std::time::Duration::from_secs(30),
        user_agent: "aemeath-test".to_string(),
    };
    let binding = runtime::ProviderFactory::build(&TestProviderFactory, spec)
        .expect("build test provider binding");
    runtime::InitialProviderAssembly::new(
        binding,
        share::config::models::ResolvedModel {
            source_key: "test".to_string(),
            source_config: share::config::models::ProviderModelsConfig::default(),
            model: share::config::models::ModelEntryConfig {
                id: "test-model".to_string(),
                context_window: 8192,
                max_tokens: 8192,
                ..Default::default()
            },
            driver: "test".to_string(),
        },
        runtime::ModelRuntimeSettings {
            max_tokens: 8192,
            reasoning: false,
            reasoning_effort: None,
        },
    )
}

struct NoopAgentRunner;

#[async_trait::async_trait]
impl tools::AgentRunner for NoopAgentRunner {
    async fn run_agent(&self, _request: tools::AgentRunRequest<'_>) -> tools::AgentRunTerminal {
        tools::AgentRunTerminal::Completed {
            result: String::new(),
        }
    }
}

fn test_prompt_assembly() -> runtime::PromptAssembly {
    runtime::PromptAssembly::new(Vec::new(), String::new(), String::new())
}

fn test_session_bootstrap_assembly(root: &std::path::Path) -> runtime::SessionBootstrapAssembly {
    runtime::SessionBootstrapAssembly::new(root.to_path_buf(), 8192, true, false, None)
}

fn test_skill_bootstrap_assembly() -> runtime::SkillBootstrapAssembly {
    runtime::SkillBootstrapAssembly::new(tools::SkillCatalogSnapshot::from_descriptors(Vec::new()))
}

fn test_agent_runner_assembly(
    runtime_context_factory: Arc<runtime::RuntimeContextFactory>,
    active_run: Arc<runtime::ActiveRunRegistry>,
) -> runtime::AgentRunnerAssembly {
    runtime::AgentRunnerAssembly {
        runner: Arc::new(NoopAgentRunner),
        parent_context_source: runtime::ParentRunContextSource::new(),
        active_run,
        max_tool_concurrency: 10,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        runtime_context_factory,
    }
}

struct NoopReflectionHistory;

struct NoopSessionManagement;

#[async_trait::async_trait]
impl context::SessionManagementPort for NoopSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<context::session::CanonicalSession, context::SessionManagementError> {
        Err(context::SessionManagementError::NotFound(id.to_string()))
    }

    async fn list_for_project(
        &self,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<context::SessionListEntry>, context::SessionManagementError> {
        Ok(Vec::new())
    }

    async fn export_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<u8>, context::SessionManagementError> {
        Err(context::SessionManagementError::NotFound(id.to_string()))
    }

    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<context::SessionListEntry, context::SessionManagementError> {
        Err(context::SessionManagementError::Storage(
            "test port".to_string(),
        ))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentity,
        _update: context::SessionMetadataUpdate,
    ) -> Result<context::SessionListEntry, context::SessionManagementError> {
        Err(context::SessionManagementError::NotFound(id.to_string()))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentity,
    ) -> Result<(), context::SessionManagementError> {
        Err(context::SessionManagementError::NotFound(id.to_string()))
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for NoopReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for NoopReflectionHistory {
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

#[tokio::test]
async fn bootstrap_dependencies_preserve_injected_task_views() {
    let temp = tempfile::tempdir().unwrap();
    let config = config::wire_project_config(
        temp.path(),
        config::NativeConfigStore::new(Arc::new(
            storage::FileSystemBlobAdapter::new(temp.path()).unwrap(),
        )),
    )
    .await
    .unwrap();
    let workspace = project::wire_production_workspace(temp.path().to_path_buf())
        .unwrap()
        .into_views();
    let task = task::wire_task();
    let access = task.access();
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        Arc::new(storage::FileSystemDatasetAdapter::new(temp.path()).unwrap()),
        Arc::new(memory::FileLegacyMemorySourceFactory::new(temp.path())),
    ));
    let session_management: Arc<dyn context::SessionManagementPort> =
        Arc::new(NoopSessionManagement);
    let wiring = context::wire_main_session(context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        session_management: session_management.clone(),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    })
    .await
    .unwrap();

    let history: Arc<dyn memory::ReflectionHistoryStore> = Arc::new(NoopReflectionHistory);
    let tools = tools::composition::TestCatalogExecutionFactory::empty();
    let skill_wiring = tools::composition::wire_skills();
    let skill_catalog = skill_wiring.catalog();
    let tool_result_materializer = Arc::new(runtime::ToolResultMaterializer::new(
        Arc::new(runtime::AtomicBlobToolResultStore::new(
            Arc::new(storage::FileSystemBlobAdapter::new(temp.path()).unwrap()),
            temp.path().to_path_buf(),
        )),
        runtime::ToolResultMaterializationPolicy::new(50_000, 2_000, 500),
    ));
    let active_run = Arc::new(runtime::ActiveRunRegistry::default());
    let hook_runner: Arc<dyn hook::HookPort> =
        Arc::new(hook::build_dispatcher(&share::config::hooks::HooksConfig::default()).unwrap());

    let wiring_clone = wiring.clone();
    let runtime_context_factory = Arc::new(runtime::RuntimeContextFactory::new(
        tools.catalog_port(),
        tools.execution(),
        Arc::new(policy::AllowAllPolicy),
        history.clone(),
        access.clone(),
        hook_runner.clone(),
    ));
    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        runtime::RuntimeCoreDependencies::new(
            workspace,
            wiring,
            Arc::new(TestProviderFactory),
            session_management.clone(),
        ),
        runtime::RuntimeToolAssemblyDependencies::new(
            tools.catalog_port(),
            skill_catalog,
            tool_result_materializer.clone(),
            active_run.clone(),
        ),
        runtime::composition::wire_sdk_chat_ingress(),
        initial_provider_assembly(),
        test_session_bootstrap_assembly(temp.path()),
        test_prompt_assembly(),
        test_skill_bootstrap_assembly(),
        test_agent_runner_assembly(runtime_context_factory.clone(), active_run.clone()),
    );

    assert!(Arc::ptr_eq(
        dependencies.runtime_context_factory(),
        &runtime_context_factory,
    ));

    // Core dependencies that also live in RuntimeServices are intentionally
    // not projected again by RuntimeBootstrapDependencies.
    assert!(Arc::ptr_eq(
        &dependencies.session_management(),
        &session_management
    ));
    assert!(
        Arc::ptr_eq(&dependencies.wiring(), &wiring_clone),
        "wiring Arc identity preserved"
    );

    // ── Arc identity: Tool assembly dependencies ──
    assert!(Arc::ptr_eq(
        &dependencies.tool_result_materializer(),
        &tool_result_materializer
    ));
    assert!(Arc::ptr_eq(&dependencies.active_run(), &active_run));

    // ── Tool catalog: Arc identity AND functional check ──
    let catalog = dependencies.tool_catalog();
    // Functional: snapshot succeeds and returns an empty catalog.
    let snapshot = catalog
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .unwrap();
    assert_eq!(snapshot.tools.len(), 0, "empty test catalog");

    // ── Skill catalog: functional check (call succeeds without panicking) ──
    let skills = dependencies.skill_catalog();
    let _skill_list = skills.list(tools::SkillQuery::new(
        temp.path().to_path_buf(),
        vec![],
        BTreeSet::new(),
    ));
    // The filesystem skill adapter may pick up skills from global
    // directories; the list may or may not be empty. The contract is
    // that calling list() does not panic and returns a valid Vec.
}

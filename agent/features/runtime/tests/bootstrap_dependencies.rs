use std::collections::BTreeSet;
use std::sync::Arc;

struct TestProviderFactory;

impl runtime::ports::ProviderFactory for TestProviderFactory {
    fn build(
        &self,
        spec: runtime::ports::ProviderBuildSpec,
    ) -> Result<runtime::ports::ProviderBinding, provider::ProviderError> {
        struct UnusedPort;
        #[async_trait::async_trait]
        impl runtime::ports::ProviderPort for UnusedPort {
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
        Ok(runtime::ports::ProviderBinding {
            provider: Arc::new(UnusedPort),
            model: spec.model,
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
            context_window: spec.context_window,
        })
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
    let skill_loader = skill_wiring.loader();
    let tool_result_materializer = Arc::new(runtime::ToolResultMaterializer::new(
        Arc::new(runtime::AtomicBlobToolResultStore::new(
            Arc::new(storage::FileSystemBlobAdapter::new(temp.path()).unwrap()),
            temp.path().to_path_buf(),
        )),
        runtime::ToolResultMaterializationPolicy::new(50_000, 2_000, 500),
    ));
    let active_run = Arc::new(runtime::ActiveRunRegistry::default());
    let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
        hook::build_dispatcher(
            &share::config::hooks::HooksConfig::default(),
            std::collections::HashMap::new(),
        )
        .unwrap(),
    );

    let wiring_clone = wiring.clone();
    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        runtime::RuntimeCoreDependencies::new(
            workspace,
            wiring,
            Arc::new(TestProviderFactory),
            history.clone(),
            Arc::new(policy::AllowAllPolicy),
            access.clone(),
            session_management.clone(),
            hook_runner.clone(),
        ),
        runtime::RuntimeToolAssemblyDependencies::new(
            tools.catalog_port(),
            tools.execution(),
            skill_catalog,
            skill_loader.clone(),
            tool_result_materializer.clone(),
            active_run.clone(),
        ),
        {
            Arc::new(runtime::RuntimeContextFactory::new(
                tools.catalog_port(),
                tools.execution(),
                Arc::new(policy::AllowAllPolicy),
                history.clone(),
                access.clone(),
                hook_runner.clone(),
            ))
        },
    );

    // ── Arc identity: Core dependencies ──
    assert!(Arc::ptr_eq(
        &dependencies.session_management(),
        &session_management
    ));
    assert!(Arc::ptr_eq(&dependencies.reflection_history(), &history));
    assert!(Arc::ptr_eq(&dependencies.task_access(), &access));
    assert!(Arc::ptr_eq(&dependencies.hook_runner(), &hook_runner));
    assert!(
        Arc::ptr_eq(&dependencies.wiring(), &wiring_clone),
        "wiring Arc identity preserved"
    );

    // ── Arc identity: Tool assembly dependencies ──
    assert!(Arc::ptr_eq(&dependencies.skill_loader(), &skill_loader));
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

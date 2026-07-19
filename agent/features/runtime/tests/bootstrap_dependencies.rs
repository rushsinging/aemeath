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

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for NoopReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionRecord>, memory::api::MemoryError> {
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
    let config = config::wire_project_config(temp.path()).await.unwrap();
    let workspace = project::wire_production_workspace(temp.path().to_path_buf())
        .unwrap()
        .into_views();
    let task = task::wire_task();
    let access = task.access();
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        Arc::new(storage::FileSystemDatasetAdapter::new(temp.path()).unwrap()),
        Arc::new(memory::FileLegacyMemorySourceFactory::new(temp.path())),
    ));
    let wiring = context::wire_main_session(context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::NoOpCanonicalSessionWriter),
        )),
    })
    .await
    .unwrap();

    let history: Arc<dyn memory::ReflectionHistoryStore> = Arc::new(NoopReflectionHistory);

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        Arc::new(TestProviderFactory),
        tools::wire_tools(),
        history.clone(),
        Arc::new(policy::AllowAllPolicy),
        access.clone(),
    );

    assert!(Arc::ptr_eq(&dependencies.reflection_history(), &history));
    assert!(Arc::ptr_eq(&dependencies.task_access(), &access));
}

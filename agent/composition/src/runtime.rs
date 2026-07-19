pub type AgentArgs = sdk::ChatBootstrapArgs;

use std::sync::Arc;

use memory::api as memory_api;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    let identity = workspace.read().project_identity();
    let memory_config = config.reader().committed_snapshot().memory().clone();
    let legacy_base_dir = share::config::paths::global_memory_dir();
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(&legacy_base_dir)
            .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
    );
    let project_key = memory_api::ProjectMemoryKey::derive(
        &identity.initial_cwd,
        identity.git_common_dir.as_deref(),
    )
    .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let store =
        memory_api::AtomicDatasetMemoryStore::new(dataset_adapter.clone(), project_key.clone());
    let reflection_history: Arc<dyn memory_api::ReflectionHistoryStore> = Arc::new(
        memory_api::AtomicDatasetReflectionHistoryStore::new(dataset_adapter, project_key.clone()),
    );
    let legacy_factory = memory::FileLegacyMemorySourceFactory::new(legacy_base_dir);
    let legacy = memory::LegacyMemorySourceFactory::create_for(&legacy_factory, &project_key);
    let _main_memory: Arc<dyn memory_api::MemoryPort> = Arc::new(
        memory_api::ProjectMemoryOpener::new(store, legacy)
            .open(memory_api::MemoryPolicy {
                max_entries: memory_config.max_entries,
                similarity_threshold: memory_config.similarity_threshold,
            })
            .await
            .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
    );

    let task_wiring = task::wire_task();
    let session_blob = storage::api::file_system_blob(share::config::paths::global_agents_dir())
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let deps = context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener: Box::new(memory::DatasetMemoryOpener::new(
            Arc::new(
                storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
                    .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
            ),
            Arc::new(memory::FileLegacyMemorySourceFactory::new(
                share::config::paths::global_memory_dir(),
            )),
        )),
        context_factory: Arc::new(context::adapters::ProductionMainContextFactory::new(
            Arc::new(context::adapters::AtomicBlobCanonicalSessionWriter::new(
                session_blob,
            )),
        )),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;

    // Preserve the injected legacy gateway seam until #914 retires it. Runtime
    // receives only Catalog/Execution ports and never observes this registry.
    struct WiringMemoryPortSource {
        wiring: Arc<context::MainSessionWiring>,
    }
    impl tools::MemoryPortSource for WiringMemoryPortSource {
        fn current(&self) -> Arc<dyn memory::MemoryPort> {
            self.wiring.committed_memory()
        }
    }
    let registry = gateways.tools.new_registry();
    gateways.tools.register_all_tools(
        &registry,
        task_wiring.access(),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        Arc::new(WiringMemoryPortSource {
            wiring: wiring.clone(),
        }),
        workspace.control(),
    );

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        gateways.provider,
        gateways.tools,
        reflection_history,
        gateways.policy,
        task_wiring.access(),
        context::compose_session_task_capture(task_wiring.persist()),
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

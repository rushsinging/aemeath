pub type AgentArgs = sdk::ChatBootstrapArgs;

use std::sync::Arc;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    // Task BC wiring: Composition owns the single backing and its persistence envelope.
    let task_wiring = task::wire_task();
    let task_access = task_wiring.access();
    let session_tasks = context::compose_session_task_capture(task_wiring.persist());

    // Production Memory opener: FileSystemDatasetAdapter (atomic dataset store)
    // + FileLegacyMemorySourceFactory (legacy JSON migration). Composition owns
    // the concrete adapters; Context only sees the MemoryOpener port.
    let dataset_adapter = Arc::new(
        storage::FileSystemDatasetAdapter::new(share::config::paths::global_agents_dir())
            .map_err(|e| sdk::SdkError::Init(e.to_string()))?,
    );
    let legacy_factory = Arc::new(memory::FileLegacyMemorySourceFactory::new(
        share::config::paths::global_memory_dir(),
    ));
    let memory_opener = Box::new(memory::DatasetMemoryOpener::new(
        dataset_adapter,
        legacy_factory,
    ));

    // Main Session coordinator — cross-BC wiring for Runtime bootstrap.
    // Context eager-opens the initial MemoryPort from the workspace
    // ProjectIdentity + committed config MemoryConfig.
    let deps = context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener,
    };
    let wiring = context::wire_main_session(deps)
        .await
        .map_err(|e| sdk::SdkError::Init(e.to_string()))?;

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        gateways.provider,
        gateways.tools,
        task_access,
        session_tasks,
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

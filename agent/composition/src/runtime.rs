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
    let project_key = memory_api::ProjectMemoryKey::derive(
        &identity.initial_cwd,
        identity.git_common_dir.as_deref(),
    )
    .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let reflection_history: Arc<dyn memory_api::ReflectionHistoryStore> =
        Arc::new(memory_api::AtomicDatasetReflectionHistoryStore::new(
            Arc::new(
                storage::FileSystemDatasetAdapter::new(share::config::paths::global_memory_dir())
                    .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
            ),
            project_key,
        ));

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
        context_factory: Arc::new(
            context::adapters::ProductionMainContextFactory::new(Arc::new(
                context::adapters::AtomicBlobCanonicalSessionWriter::new(session_blob),
            ))
            .with_skill_supplier(
                tools::composition::wire_skill_materialization(),
                Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                    workspace.read(),
                )),
            ),
        ),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        gateways.provider,
        reflection_history,
        gateways.policy,
        task_wiring.access(),
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

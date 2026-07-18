pub type AgentArgs = sdk::ChatBootstrapArgs;

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use memory::api as memory_api;

use crate::app::FeatureGateways;

struct FileLegacyMemorySource {
    base_dir: PathBuf,
    project_file_name: String,
}

impl FileLegacyMemorySource {
    fn new(base_dir: PathBuf, initial_cwd: &str) -> Self {
        Self {
            project_file_name: storage::project_file_name_from_path(std::path::Path::new(
                initial_cwd,
            )),
            base_dir,
        }
    }

    fn paths(&self, layer: memory_api::MemoryLayer) -> (PathBuf, PathBuf) {
        match layer {
            memory_api::MemoryLayer::Global => (
                self.base_dir.join("_global.json"),
                self.base_dir.join("_global_archive.json"),
            ),
            memory_api::MemoryLayer::Project => (
                self.base_dir
                    .join(format!("{}.json", self.project_file_name)),
                self.base_dir
                    .join(format!("{}_archive.json", self.project_file_name)),
            ),
        }
    }

    fn read_member(
        path: &std::path::Path,
    ) -> Result<memory_api::LegacyMemoryMember, memory_api::LegacyMemorySourceError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(memory_api::LegacyMemoryMember::Present(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(memory_api::LegacyMemoryMember::Missing)
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(memory_api::LegacyMemorySourceError::PermissionDenied)
            }
            Err(_) => Err(memory_api::LegacyMemorySourceError::Io),
        }
    }
}

#[async_trait]
impl memory_api::LegacyMemorySource for FileLegacyMemorySource {
    async fn probe(
        &self,
        layer: memory_api::MemoryLayer,
    ) -> Result<memory_api::LegacyMemoryLayer, memory_api::LegacyMemorySourceError> {
        let (active, archive) = self.paths(layer);
        Ok(memory_api::LegacyMemoryLayer {
            active: Self::read_member(&active)?,
            archive: Self::read_member(&archive)?,
        })
    }
}

pub(crate) use runtime::AgentClientImpl;

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
) -> Result<AgentClientImpl, sdk::SdkError> {
    let identity = workspace.read().project_identity();
    let memory_config = config.reader().committed_snapshot().memory().clone();
    let legacy_base_dir = storage::memory_base_dir();
    let dataset_adapter = storage::FileSystemDatasetAdapter::new(&legacy_base_dir)
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let project_key = memory_api::ProjectMemoryKey::derive(
        &identity.initial_cwd,
        identity.git_common_dir.as_deref(),
    )
    .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let store = memory_api::AtomicDatasetMemoryStore::new(Arc::new(dataset_adapter), project_key);
    let legacy = Arc::new(FileLegacyMemorySource::new(
        legacy_base_dir,
        &identity.initial_cwd,
    ));
    let main_memory: Arc<dyn memory_api::MemoryPort> = Arc::new(
        memory_api::ProjectMemoryOpener::new(store, legacy)
            .open(memory_api::MemoryPolicy {
                max_entries: memory_config.max_entries,
                similarity_threshold: memory_config.similarity_threshold,
            })
            .await
            .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
    );

    let task_wiring = task::wire_task();
    // Consume the injected gateway at the composition boundary. Runtime receives
    // only the catalog/execution ports assembled by its current bootstrap seam.
    let registry = gateways.tools.new_registry();
    gateways.tools.register_all_tools(
        &registry,
        task_wiring.access(),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        workspace.control(),
    );
    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        workspace,
        runtime::RuntimeConfigDependencies::new(config.reader(), config.query(), config.writer()),
        main_memory,
        gateways.provider,
        gateways.tools,
        gateways.policy,
        task_wiring.access(),
        context::compose_session_task_capture(task_wiring.persist()),
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

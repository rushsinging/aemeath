pub type AgentArgs = sdk::ChatBootstrapArgs;

use std::sync::Arc;

use memory::api as memory_api;

use crate::app::FeatureGateways;

pub(crate) use runtime::AgentClientImpl;

struct WiringMemoryPortSource {
    wiring: Arc<context::MainSessionWiring>,
}

impl tools::MemoryPortSource for WiringMemoryPortSource {
    fn current(&self) -> Arc<dyn memory::MemoryPort> {
        self.wiring.committed_memory()
    }
}

struct RuntimeToolAssembly {
    catalog: Arc<dyn tools::ToolCatalogPort>,
    execution: Arc<dyn tools::ToolExecutionPort>,
    binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    tool_result_materializer: Arc<runtime::ToolResultMaterializer>,
    active_run: Arc<runtime::ActiveRunRegistry>,
}

fn wire_runtime_tool_assembly(
    task_access: Arc<dyn task::TaskAccess>,
    memory_source: Arc<dyn tools::MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    snapshot: &share::config::domain::snapshot::ConfigSnapshot,
    agents_dir: &std::path::Path,
) -> Result<RuntimeToolAssembly, sdk::SdkError> {
    let tools = tools::composition::wire_builtin_catalog_execution(
        task_access,
        memory_source,
        workspace_control,
    )
    .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let policy = snapshot.tool_result_policy();
    let agents_dir_buf = agents_dir.to_path_buf();
    let blobs = Arc::new(runtime::AtomicBlobToolResultStore::new(
        Arc::new(
            storage::FileSystemBlobAdapter::new(agents_dir_buf.clone())
                .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
        ),
        agents_dir_buf,
    ));
    Ok(RuntimeToolAssembly {
        catalog: tools.catalog(),
        execution: tools.execution(),
        binding: tools.binding(),
        tool_result_materializer: Arc::new(runtime::ToolResultMaterializer::new(
            blobs,
            runtime::ToolResultMaterializationPolicy::new(
                policy.threshold_chars(),
                policy.preview_head_chars(),
                policy.preview_tail_chars(),
            ),
        )),
        active_run: Arc::new(runtime::ActiveRunRegistry::default()),
    })
}

pub(crate) async fn from_args_with_gateways(
    args: AgentArgs,
    gateways: FeatureGateways,
    workspace: project::WorkspaceViews,
    config: config::ConfigWiring,
    agents_dir: &std::path::Path,
) -> Result<AgentClientImpl, sdk::SdkError> {
    let identity = workspace.read().project_identity();
    let project_key = memory_api::ProjectMemoryKey::derive(
        &identity.initial_cwd,
        identity.git_common_dir.as_deref(),
    )
    .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    // StorageNamespace::Memory adds "memory" segment → adapter root is
    // agents_dir so the dataset lives at agents_dir/memory/{project}/...
    // (not agents_dir/memory/memory/...). Legacy memory still uses the
    // explicit agents_dir.join("memory") path via FileLegacyMemorySourceFactory.
    let reflection_history: Arc<dyn memory_api::ReflectionHistoryStore> =
        Arc::new(memory_api::AtomicDatasetReflectionHistoryStore::new(
            Arc::new(
                storage::FileSystemDatasetAdapter::new(agents_dir)
                    .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
            ),
            project_key,
        ));

    let task_wiring = task::wire_task();
    let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
        hook::build_dispatcher(
            config.reader().committed_snapshot().hooks(),
            std::collections::HashMap::new(),
        )
        .map_err(|errors| sdk::SdkError::Init(format!("Hook 配置初始化失败：{errors:?}")))?,
    );
    let skill_wiring = tools::composition::wire_skills();
    let skill_catalog = skill_wiring.catalog();
    let skill_materializer = skill_wiring.materializer();
    let session_blob = storage::api::file_system_blob(agents_dir)
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let session_management: Arc<dyn context::SessionManagementPort> = Arc::new(
        context::adapters::AtomicBlobSessionManagement::new(session_blob.clone()),
    );
    let agents_dir_buf = agents_dir.to_path_buf();
    let deps = context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task_wiring.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener: Box::new(memory::DatasetMemoryOpener::new(
            Arc::new(
                storage::FileSystemDatasetAdapter::new(agents_dir_buf)
                    .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
            ),
            Arc::new(memory::FileLegacyMemorySourceFactory::new(
                agents_dir.join("memory"),
            )),
        )),
        session_management: session_management.clone(),
        context_factory: Arc::new(
            context::adapters::ProductionMainContextFactory::new(Arc::new(
                context::adapters::AtomicBlobCanonicalSessionWriter::new(session_blob),
            ))
            .with_skill_supplier(
                skill_materializer.clone(),
                Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                    workspace.read(),
                )),
            ),
        ),
    };
    let wiring = context::wire_main_session(deps)
        .await
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;

    let tool_assembly = wire_runtime_tool_assembly(
        task_wiring.access(),
        Arc::new(WiringMemoryPortSource {
            wiring: wiring.clone(),
        }),
        workspace.control(),
        &config.reader().committed_snapshot(),
        agents_dir,
    )?;

    // #1248 Task 3: Construct RuntimeContextFactory via its narrow crate-root
    // entry — seven explicit port parameters, no opaque RuntimeServices bag.
    let runtime_context_factory = Arc::new(runtime::RuntimeContextFactory::new(
        tool_assembly.catalog.clone(),
        tool_assembly.execution.clone(),
        tool_assembly.binding.clone(),
        gateways.policy.clone(),
        reflection_history.clone(),
        task_wiring.access(),
        hook_runner.clone(),
    ));

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        runtime::RuntimeCoreDependencies::new(
            workspace,
            wiring,
            gateways.provider,
            reflection_history,
            gateways.policy,
            task_wiring.access(),
            session_management,
            hook_runner,
        ),
        runtime::RuntimeToolAssemblyDependencies::new(
            tool_assembly.catalog,
            tool_assembly.execution,
            tool_assembly.binding,
            skill_catalog,
            skill_materializer,
            tool_assembly.tool_result_materializer,
            tool_assembly.active_run,
        ),
        runtime_context_factory,
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

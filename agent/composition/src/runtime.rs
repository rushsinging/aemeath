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
    tool_result_materializer: Arc<runtime::ToolResultMaterializer>,
    active_run: Arc<runtime::ActiveRunRegistry>,
}

fn wire_runtime_tool_assembly(
    task_access: Arc<dyn task::TaskAccess>,
    memory_source: Arc<dyn tools::MemoryPortSource>,
    workspace_control: Arc<dyn project::WorkspaceControl>,
    skill_loader: Arc<dyn tools::SkillLoadPort>,
    snapshot: &share::config::domain::snapshot::ConfigSnapshot,
    agents_dir: &std::path::Path,
) -> Result<RuntimeToolAssembly, sdk::SdkError> {
    let tools = tools::composition::wire_builtin_catalog_execution(
        task_access,
        memory_source,
        workspace_control,
        skill_loader,
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
        hook::build_dispatcher(&config.reader().committed_snapshot())
            .map_err(|errors| sdk::SdkError::Init(format!("Hook 配置初始化失败：{errors:?}")))?,
    );
    let skill_wiring = tools::composition::wire_skills();
    let skill_catalog = skill_wiring.catalog();
    let skill_loader = skill_wiring.loader();
    let session_dataset = Arc::new(
        storage::FileSystemDatasetAdapter::new(agents_dir)
            .map_err(|error| sdk::SdkError::Init(error.to_string()))?,
    );
    let session_blob = storage::api::file_system_blob(agents_dir)
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let session_management: Arc<dyn context::SessionManagementPort> =
        Arc::new(context::adapters::DatasetSessionManagement::new(
            session_dataset.clone(),
            session_blob.clone(),
        ));

    let snapshot = config.reader().committed_snapshot();
    let runtime_model = snapshot
        .resolve_runtime_model(args.model.as_deref(), args.max_tokens)
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let resolved_model = runtime_model.resolved_model().clone();
    let runtime_settings = runtime::resolve_model_runtime_settings(
        runtime_model.max_tokens(),
        &resolved_model.model,
        !args.no_think,
    );
    let api_key = (!resolved_model.source_config.api_key.is_empty())
        .then(|| resolved_model.source_config.api_key.clone())
        .ok_or_else(|| {
            sdk::SdkError::Init(
                "API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json".to_string(),
            )
        })?;
    let provider_spec = runtime::ProviderBuildSpec {
        driver: resolved_model.driver.clone(),
        source_key: resolved_model.source_key.clone(),
        api_style: resolved_model.model.api_style.clone(),
        api_key,
        base_url: args.base_url.clone().or_else(|| {
            (!resolved_model.source_config.base_url.is_empty())
                .then(|| resolved_model.source_config.base_url.clone())
        }),
        model: provider::ModelId {
            provider: resolved_model.source_key.clone(),
            model: resolved_model.model.id.clone(),
        },
        max_tokens: runtime_model.max_tokens(),
        requested_reasoning: runtime_settings
            .reasoning_effort
            .as_deref()
            .and_then(provider::ReasoningLevel::parse)
            .unwrap_or(if runtime_settings.reasoning {
                provider::ReasoningLevel::Medium
            } else {
                provider::ReasoningLevel::Off
            }),
        context_window: (resolved_model.model.context_window > 0)
            .then_some(resolved_model.model.context_window),
        timeout: std::time::Duration::from_secs(snapshot.api_timeout_secs()),
        user_agent: snapshot.user_agent().to_string(),
    };
    let initial_binding = gateways
        .provider
        .build(provider_spec)
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?;
    let initial_provider =
        runtime::InitialProviderAssembly::new(initial_binding, resolved_model, runtime_settings);

    // #1486：compact 走 LLM 语义压缩 —— 注入 ProviderCompactGenerator
    // （包装主 provider binding）。context_factory 依赖 initial_binding，
    // 因此 MainSession 装配延后到 provider 构建之后。
    let agents_dir_buf = agents_dir.to_path_buf();
    let compact_generator = runtime::ProviderCompactGenerator::new(
        initial_provider.binding().provider.clone(),
        initial_provider.binding().model.clone(),
    );
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
                context::adapters::DatasetCanonicalSessionWriter::new(session_dataset),
            ))
            .with_accepted_input_writer(Arc::new(
                context::adapters::AtomicBlobAcceptedInputWriter::new(session_blob.clone()),
            ))
            .with_tool_receipt_writer(Arc::new(
                context::adapters::AtomicBlobToolReceiptWriter::new(session_blob),
            ))
            .with_skill_catalog(
                skill_catalog.clone(),
                Arc::new(context::adapters::WorkspaceSkillQueryFactory::new(
                    workspace.read(),
                )),
            )
            .with_generator(Arc::new(compact_generator)),
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
        skill_loader.clone(),
        &config.reader().committed_snapshot(),
        agents_dir,
    )?;

    // 构造一次基础 RuntimeContextFactory，并将同一 Arc 注入 Main bootstrap
    // 与 Derived Agent Runner；Derived 仅追加受限 binding，不重建基础服务。
    let runtime_context_factory = Arc::new(runtime::RuntimeContextFactory::new(
        tool_assembly.catalog.clone(),
        tool_assembly.execution.clone(),
        gateways.policy.clone(),
        reflection_history.clone(),
        task_wiring.access(),
        hook_runner.clone(),
    ));

    context::guidance::init_guidance_dir();
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let context_size = snapshot.resolve_context_size(
        Some(args.context_size),
        initial_provider.resolved_model().model.context_window,
    );
    let session_bootstrap = runtime::SessionBootstrapAssembly::new(
        cwd,
        context_size,
        args.allow_all,
        args.verbose,
        args.resume.clone(),
    );

    let prompt_root = std::path::PathBuf::from(&identity.initial_cwd);
    let prompt_context = runtime::PromptContext::new(
        &prompt_root,
        Some(&initial_provider.binding().model.provider),
        Some(&initial_provider.binding().model.model),
        snapshot.permission_mode(),
    );
    let prompt_parts =
        runtime::build_system_prompt_parts(&prompt_context, &hook_runner, snapshot.language())
            .await;
    let static_prompt = runtime::build_static_prompt(
        &prompt_root,
        &initial_provider.binding().model.model,
        initial_provider.runtime_settings().reasoning,
        Some(&snapshot),
        &hook_runner,
        prompt_parts.clone(),
    )
    .await;
    let prompt = runtime::PromptAssembly::new(
        vec![provider::RequestSystemBlock::Cacheable(static_prompt)],
        prompt_parts.initial_git_context,
        prompt_parts.claude_md,
        initial_provider.binding().model.model.clone(),
    );

    let available_tools = tool_assembly
        .catalog
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .map_err(|error| sdk::SdkError::Init(error.to_string()))?
        .tools
        .iter()
        .map(|descriptor| descriptor.name.as_str().to_string())
        .collect();
    let skill_query = tools::SkillQuery::new(
        prompt_root.clone(),
        snapshot.skills().dirs.clone(),
        available_tools,
    );
    let descriptors = skill_catalog.list(skill_query);
    let skills = runtime::SkillBootstrapAssembly::new(
        tools::SkillCatalogSnapshot::from_descriptors(descriptors),
    );

    let (max_tool_concurrency, max_agent_concurrency) = runtime::resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        &snapshot,
    );
    let agent_runner = runtime::build_agent_runner(
        gateways.provider.clone(),
        tool_assembly.active_run.clone(),
        max_tool_concurrency,
        Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency)),
        tool_assembly.tool_result_materializer.clone(),
        workspace.clone(),
        skill_catalog.clone(),
        runtime::ParentRunContextSource::new(),
        runtime_context_factory.clone(),
    );

    let dependencies = runtime::RuntimeBootstrapDependencies::new(
        runtime::RuntimeCoreDependencies::new(
            workspace,
            wiring,
            gateways.provider,
            session_management,
        ),
        runtime::RuntimeToolAssemblyDependencies::new(
            tool_assembly.catalog,
            skill_catalog,
            tool_assembly.tool_result_materializer,
            tool_assembly.active_run,
        ),
        runtime::composition::wire_sdk_chat_ingress(),
        initial_provider,
        session_bootstrap,
        prompt,
        skills,
        agent_runner,
    );
    runtime::from_args_with_workspace(args, dependencies).await
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

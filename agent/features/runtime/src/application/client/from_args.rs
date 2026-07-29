use std::sync::Arc;
use std::time::Duration;

use sdk::SdkError;

use crate::application::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::application::startup::ChatBootstrapArgs;
use crate::application::startup::{
    build_agent_runner, resolve_concurrency_limits, resolve_model_runtime_settings,
};
use crate::ports::{ProviderBuildSpec, ProviderFactory, RequestSystemBlock};

use super::{AgentClientImpl, RuntimeHandle};

/// 由 Composition 装配、供 Runtime bootstrap 转发的 Tool/Skill/Run 资源。
pub struct RuntimeToolAssemblyDependencies {
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    skill_loader: Arc<dyn tools::SkillLoadPort>,
    tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
}

impl RuntimeToolAssemblyDependencies {
    pub fn new(
        tool_catalog: Arc<dyn tools::ToolCatalogPort>,
        tool_execution: Arc<dyn tools::ToolExecutionPort>,
        tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
        skill_catalog: Arc<dyn tools::SkillCatalogPort>,
        skill_loader: Arc<dyn tools::SkillLoadPort>,
        tool_result_materializer: Arc<
            crate::application::tool_result_materialization::ToolResultMaterializer,
        >,
        active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    ) -> Self {
        Self {
            tool_catalog,
            tool_execution,
            tool_context_binding,
            skill_catalog,
            skill_loader,
            tool_result_materializer,
            active_run,
        }
    }
}

/// 由 Composition 装配、供 Runtime bootstrap 转发的基础运行资源。
pub struct RuntimeCoreDependencies {
    workspace: project::WorkspaceViews,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    policy: Arc<dyn policy::PolicyPort>,
    task_access: Arc<dyn task::TaskAccess>,
    session_management: Arc<dyn context::SessionManagementPort>,
    hook_runner: Arc<dyn hook::HookPort>,
}

impl RuntimeCoreDependencies {
    pub fn new(
        workspace: project::WorkspaceViews,
        wiring: Arc<context::MainSessionWiring>,
        provider_factory: Arc<dyn ProviderFactory>,
        reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
        policy: Arc<dyn policy::PolicyPort>,
        task_access: Arc<dyn task::TaskAccess>,
        session_management: Arc<dyn context::SessionManagementPort>,
        hook_runner: Arc<dyn hook::HookPort>,
    ) -> Self {
        Self {
            workspace,
            wiring,
            provider_factory,
            reflection_history,
            policy,
            task_access,
            session_management,
            hook_runner,
        }
    }
}

/// Runtime bootstrap 所需的活依赖；由 Composition 一次性构造并注入。
///
/// #1248 Task 3: `runtime_context_factory` is constructed by Composition
/// and injected here so `from_args_with_workspace` does not construct
/// the factory itself.
pub struct RuntimeBootstrapDependencies {
    workspace: project::WorkspaceViews,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    #[allow(dead_code)]
    reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    #[allow(dead_code)]
    policy: Arc<dyn policy::PolicyPort>,
    #[allow(dead_code)]
    task_access: Arc<dyn task::TaskAccess>,
    session_management: Arc<dyn context::SessionManagementPort>,
    hook_runner: Arc<dyn hook::HookPort>,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    #[allow(dead_code)]
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    #[allow(dead_code)]
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    skill_loader: Arc<dyn tools::SkillLoadPort>,
    tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    /// #1248 Task 3: Factory assembled by Composition from RuntimeServices.
    runtime_context_factory:
        Arc<crate::application::runtime_context_factory::RuntimeContextFactory>,
}

impl RuntimeBootstrapDependencies {
    pub fn new(
        core: RuntimeCoreDependencies,
        tool_assembly: RuntimeToolAssemblyDependencies,
        runtime_context_factory: Arc<
            crate::application::runtime_context_factory::RuntimeContextFactory,
        >,
    ) -> Self {
        let RuntimeCoreDependencies {
            workspace,
            wiring,
            provider_factory,
            reflection_history,
            policy,
            task_access,
            session_management,
            hook_runner,
        } = core;
        let RuntimeToolAssemblyDependencies {
            tool_catalog,
            tool_execution,
            tool_context_binding,
            skill_catalog,
            skill_loader,
            tool_result_materializer,
            active_run,
        } = tool_assembly;
        Self {
            workspace,
            wiring,
            provider_factory,
            reflection_history,
            policy,
            task_access,
            session_management,
            hook_runner,
            tool_catalog,
            tool_execution,
            tool_context_binding,
            skill_catalog,
            skill_loader,
            tool_result_materializer,
            active_run,
            runtime_context_factory,
        }
    }

    pub fn runtime_context_factory(
        &self,
    ) -> &Arc<crate::application::runtime_context_factory::RuntimeContextFactory> {
        &self.runtime_context_factory
    }

    pub fn reflection_history(&self) -> Arc<dyn memory::api::ReflectionHistoryStore> {
        self.reflection_history.clone()
    }

    pub fn task_access(&self) -> Arc<dyn task::TaskAccess> {
        self.task_access.clone()
    }

    pub fn session_management(&self) -> Arc<dyn context::SessionManagementPort> {
        self.session_management.clone()
    }

    pub fn hook_runner(&self) -> Arc<dyn hook::HookPort> {
        self.hook_runner.clone()
    }

    pub fn wiring(&self) -> Arc<context::MainSessionWiring> {
        self.wiring.clone()
    }

    pub fn tool_catalog(&self) -> Arc<dyn tools::ToolCatalogPort> {
        self.tool_catalog.clone()
    }

    pub fn skill_catalog(&self) -> Arc<dyn tools::SkillCatalogPort> {
        self.skill_catalog.clone()
    }

    pub fn skill_loader(&self) -> Arc<dyn tools::SkillLoadPort> {
        self.skill_loader.clone()
    }

    pub fn tool_result_materializer(
        &self,
    ) -> Arc<crate::application::tool_result_materialization::ToolResultMaterializer> {
        self.tool_result_materializer.clone()
    }

    pub fn active_run(&self) -> Arc<crate::application::active_run::ActiveRunRegistry> {
        self.active_run.clone()
    }
}

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
///
/// `task_access` 由 Composition 层注入；Runtime 不得自行创建
/// Task BC 的 backing 或持久化封套（跨域越权，#890）。
pub async fn from_args_with_workspace(
    args: ChatBootstrapArgs,
    dependencies: RuntimeBootstrapDependencies,
) -> Result<AgentClientImpl, SdkError> {
    let RuntimeBootstrapDependencies {
        workspace,
        wiring,
        provider_factory,
        session_management,
        hook_runner,
        tool_catalog,
        skill_catalog,
        skill_loader: _,
        tool_result_materializer,
        active_run,
        runtime_context_factory,
        ..
    } = dependencies;

    // Config query/writer come from the wiring gate-aware façade.
    // Bootstrap reads committed_config directly from wiring (one-shot).
    let config_query = wiring.config_query();
    let config_writer = wiring.config_writer();

    // 1. Guidance 目录初始化
    context::guidance::init_guidance_dir();

    // 2. 解析 cwd
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 3. Session — startup resume is scoped to the current project identity.
    // A rejected cross-project id leaves the committed snapshot unchanged.
    let (session_id, startup_resume) = if let Some(resume_id) = args.resume.as_ref() {
        match crate::application::client::resume_helper::resume_session_to_backing(
            resume_id, &wiring,
        )
        .await
        {
            Ok(projection) => {
                log::info!(target: crate::LOG_TARGET, "startup resume: {}", projection.session_id);
                log::debug!(
                    target: crate::LOG_TARGET,
                    "resume_lifecycle boundary=startup_projection stage=created session_id={} steps={} messages={} last_step_messages={} last_message_role={} last_message_text_len={}",
                    projection.session_id,
                    projection.display_steps.len(),
                    projection.display_steps.iter().map(|step| step.messages.len()).sum::<usize>(),
                    projection.display_steps.last().map(|step| step.messages.len()).unwrap_or(0),
                    projection.display_steps.last().and_then(|step| step.messages.last()).map(|message| format!("{:?}", message.role)).unwrap_or_else(|| "-".to_string()),
                    projection.display_steps.last().and_then(|step| step.messages.last()).map(|message| message.text_content().len()).unwrap_or(0)
                );
                let session_id = projection.session_id.clone();
                let startup_resume = sdk::SessionResumeView {
                    steps: projection
                        .display_steps
                        .into_iter()
                        .map(|step| sdk::ResumedSessionStep {
                            run_id: step.run_id,
                            step_id: step.step_id,
                            messages: step
                                .messages
                                .into_iter()
                                .map(crate::application::client::message_to_sdk)
                                .collect(),
                        })
                        .collect(),
                    session_id: projection.session_id,
                    created_at: chrono::DateTime::parse_from_rfc3339(&projection.created_at)
                        .map(|dt| dt.timestamp_millis() as u64)
                        .unwrap_or(0),
                };
                (session_id, Some(startup_resume))
            }
            Err(error) => {
                return Err(SdkError::Init(format!(
                    "startup resume of session {resume_id} failed: {error}"
                )));
            }
        }
    } else {
        // Non-resume: use the wiring's committed session id so Runtime
        // and the Context coordinator share the same canonical session.
        let session_id = wiring.committed_session().id.clone();
        log::info!(target: crate::LOG_TARGET, "session started");
        (session_id, None)
    };
    // Session id determined above; committed_config remains bound to the
    // current project because cross-project resume is rejected.

    // 4. Read the current committed config snapshot.
    let snapshot = wiring.committed_config();

    // 5. 日志已由 Composition 在进入 Runtime 前初始化。

    // 6. 模型选择与运行参数解析 — 由 ConfigSnapshot 收敛 config 语义。
    let runtime_model = snapshot
        .resolve_runtime_model(args.model.as_deref(), args.max_tokens)
        .map_err(|e| SdkError::Init(e.to_string()))?;
    let resolved_model = runtime_model.resolved_model().clone();
    let driver = resolved_model.driver.as_str();
    // 8. API key
    let api_key = non_empty_string(&resolved_model.source_config.api_key).ok_or_else(|| {
        SdkError::Init(
            "API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json".to_string(),
        )
    })?;

    // 9. Base URL + model + runtime settings
    let base_url = args
        .base_url
        .clone()
        .or_else(|| non_empty_string(&resolved_model.source_config.base_url));
    let model = resolved_model.model.id.clone();
    let runtime_settings = resolve_model_runtime_settings(
        runtime_model.max_tokens(),
        &resolved_model.model,
        !args.no_think,
    );

    log::info!(target: crate::LOG_TARGET,
        "[main] source={} api={} model={} reasoning={} args.no_think={}",
        resolved_model.source_key,
        driver,
        model,
        runtime_settings.reasoning,
        args.no_think
    );

    let spec = ProviderBuildSpec {
        driver: driver.to_string(),
        source_key: resolved_model.source_key.clone(),
        api_style: resolved_model.model.api_style.clone(),
        api_key,
        base_url,
        model: provider::ModelId {
            provider: resolved_model.source_key.clone(),
            model: model.clone(),
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
        timeout: Duration::from_secs(snapshot.api_timeout_secs()),
        user_agent: snapshot.user_agent().to_string(),
    };
    let binding = provider_factory
        .build(spec)
        .map_err(|error| SdkError::Init(error.to_string()))?;

    // 11. Tooling
    let available_tools = tool_catalog
        .snapshot_for_run(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
            &snapshot.tool_selection(),
        )
        .map_err(|error| SdkError::Init(error.to_string()))?
        .tools
        .iter()
        .map(|descriptor| descriptor.name.as_str().to_string())
        .collect();
    let skill_query =
        tools::SkillQuery::new(cwd.clone(), snapshot.skills().dirs.clone(), available_tools);
    let descriptors = skill_catalog.list(skill_query);
    let initial_skill_snapshot = tools::SkillCatalogSnapshot::from_descriptors(descriptors);
    // #1327 承接 MCP Ready lifecycle / Catalog 同步；#1294 不保留 MCP manager 或
    // Tools 私有 CatalogExecutionWiring 接线。

    // 12. Hook runner 由 Composition 注入，Main/Sub 共享同一实例。

    // 13. Tool Result materializer 与 14. active-run registry 由 Composition 注入。

    // 15. Concurrency limits — must resolve before building agent runner.
    let (max_tool_concurrency, max_agent_concurrency) = resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        &snapshot,
    );
    let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));
    log::info!(target: crate::LOG_TARGET,
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 16. PolicyPort 已由 Composition 注入；同一 Arc 分发给 Main 与 Sub。

    // 17. #1385 Task 7: Memory port is obtained per-run via BoundMainRun
    // (assemble_main_runtime_context), not at bootstrap time.

    // #1385 Task 6: shared parent context source — created once and injected
    // into both CliAgentRunner and MainSessionShell so that the Main Run loop
    // can install the current RuntimeContext before tool execution and sub-agent
    // derivation reads it.
    let parent_context_source = crate::application::runtime_context::ParentRunContextSource::new();

    // #1248 Task 3: RuntimeContextFactory is constructed by Composition and
    // injected through RuntimeBootstrapDependencies.  No need to construct
    // RuntimeServices or the factory here.

    let agent_runner = build_agent_runner(
        wiring.config_reader(),
        provider_factory.clone(),
        active_run.clone(),
        max_tool_concurrency,
        agent_semaphore.clone(),
        tool_result_materializer.clone(),
        workspace.clone(),
        skill_catalog.clone(),
        parent_context_source.clone(),
        runtime_context_factory.clone(),
    );

    // 18. Prompt bundle
    let prompt_context = PromptContext::new(
        &cwd,
        Some(&binding.model.provider),
        Some(&binding.model.model),
        snapshot.permission_mode(),
    );
    let prompt_parts =
        build_system_prompt_parts(&prompt_context, &hook_runner, snapshot.language()).await;

    let static_prompt = crate::application::prompt::prompt_build_ext::build_static_prompt(
        &cwd,
        &model,
        runtime_settings.reasoning,
        Some(&snapshot),
        &hook_runner,
        prompt_parts.clone(),
    )
    .await;
    let system_blocks = vec![RequestSystemBlock::Cacheable(static_prompt)];
    let system_prompt_text = system_blocks
        .iter()
        .map(RequestSystemBlock::text)
        .collect::<Vec<_>>()
        .join("\n\n");

    // 19. Concurrency
    log::info!(
        target: crate::LOG_TARGET,
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 20. context_size
    let context_size =
        snapshot.resolve_context_size(Some(args.context_size), resolved_model.model.context_window);

    let memory_config = snapshot.memory().clone();

    // 20b. #1385: 构建 MainSessionShell（session 级状态，§2.2）
    let shell = crate::application::client::accessors::MainSessionShell {
        session_id: session_id.clone(),
        cwd: cwd.clone(),
        workspace: workspace.clone(),
        wiring: wiring.clone(),
        config_query: config_query.clone(),
        config_writer: config_writer.clone(),
        session_management: session_management.clone(),
        provider_factory: provider_factory.clone(),
        resolved_model: resolved_model.clone(),
        current_binding: Arc::new(std::sync::RwLock::new(Arc::new(binding.clone()))),
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore: agent_semaphore.clone(),
        system_blocks,
        system_prompt_text,
        initial_git_context: prompt_parts.initial_git_context,
        user_context: prompt_parts.claude_md,
        // ── Skills ──
        skill_catalog: skill_catalog.clone(),
        initial_skill_snapshot,
        memory_config,
        context_size,
        language: snapshot.language().to_string(),
        allow_all: args.allow_all,
        verbose: args.verbose,
        resume: args.resume,
        startup_resume,
        agent_runner,
        parent_context_source,
        tool_result_materializer,
        active_run: active_run.clone(),
        interaction_bridge: Arc::new(crate::application::interaction::InteractionBridge::new()),
        event_sink_factory: Arc::new(|tx| {
            crate::application::main_loop::ChatEventSinkHandle::new(
                crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
            )
        }),
        input_port_factory: Arc::new(|queue, events| super::accessors::InputPortPair {
            queue: crate::adapters::input_buffer::RuntimeQueueDrainPort::new(queue),
            input_events: crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(events),
        }),
        session_reminders: Arc::new(std::sync::RwLock::new(
            share::memory::SessionReminders::new(),
        )),
        runtime_context_factory,
    };

    // 21. 构建 handle — #1385 Task 7: shell is the single source.
    let handle = RuntimeHandle { shell };

    Ok(AgentClientImpl {
        inner: Arc::new(handle),
    })
}

// ─── 内部辅助 ───

fn non_empty_string(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::application::client::accessors::MainSessionShell;
    use crate::application::run_config::RunConfigSnapshot;
    use crate::domain::agent_run::RunSpec;
    use crate::ports::{ContextPort, PolicyPort};
    use hook::{HookInvocation, HookOutcome, HookPort};
    use memory::api::{MemoryPort, ReflectionHistoryStore};

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// #1385 Task 12: noop event sink handle for tests.
    fn test_sink() -> crate::application::main_loop::ChatEventSinkHandle {
        #[derive(Clone)]
        struct NoOpSink;
        impl crate::application::main_loop::ChatEventSink for NoOpSink {
            fn send_event<'a>(
                &'a self,
                _event: crate::application::main_loop::RuntimeStreamEvent,
            ) -> crate::application::main_loop::EventFuture<'a> {
                Box::pin(std::future::ready(()))
            }
            fn try_send_event(&self, _event: crate::application::main_loop::RuntimeStreamEvent) {}
        }
        crate::application::main_loop::ChatEventSinkHandle::new(NoOpSink)
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

    // ── Fake implementations for RuntimeContext assembly tests ──

    struct FakeContextPort;
    #[async_trait::async_trait]
    impl ContextPort for FakeContextPort {
        async fn build_window(
            &self,
            _request: &crate::ports::ContextRequest,
        ) -> Result<crate::ports::ContextWindow, crate::ports::ContextPortError> {
            Err(crate::ports::ContextPortError::Compact("fake".into()))
        }
        async fn needs_compaction(
            &self,
            _request: &crate::ports::ContextRequest,
        ) -> Result<crate::ports::CompactionDecision, crate::ports::ContextPortError> {
            Err(crate::ports::ContextPortError::Compact("fake".into()))
        }
        async fn compact(
            &self,
            _request: &crate::ports::CompactRequest,
        ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
            Err(crate::ports::ContextPortError::Compact("fake".into()))
        }
        async fn manual_compact(
            &self,
            _request: &crate::ports::ManualCompactRequest,
        ) -> Result<crate::ports::CompactOutcome, crate::ports::ContextPortError> {
            Err(crate::ports::ContextPortError::Compact("fake".into()))
        }
        async fn clear_session(
            &self,
            _session_id: &crate::ports::SessionId,
        ) -> Result<(), crate::ports::ContextPortError> {
            Ok(())
        }
        async fn append_and_persist(
            &self,
            _append: &crate::ports::ContextAppend,
        ) -> Result<crate::ports::AppendReceipt, crate::ports::ContextAppendError> {
            Err(crate::ports::ContextAppendError::Storage("fake".into()))
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
            _cancellation: &tokio_util::sync::CancellationToken,
        ) -> HookOutcome {
            HookOutcome::proceed()
        }
    }
    /// Build a minimal `MainSessionShell` with fake ports for assembler tests.
    /// Accepts a shared `ParentRunContextSource` so tests that wire up both
    /// a shell and a runner pass the same source — no orphan source.
    async fn make_test_shell(
        parent_context_source: crate::application::runtime_context::ParentRunContextSource,
    ) -> MainSessionShell {
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
        let binding = crate::application::testing::test_binding(Vec::new());
        let policy: Arc<dyn PolicyPort> = Arc::new(policy::AllowAllPolicy);
        let _memory: Arc<dyn MemoryPort> = Arc::new(memory::NoOpMemory);
        let tools_factory = tools::composition::TestCatalogExecutionFactory::empty();
        let tool_catalog: Arc<dyn tools::ToolCatalogPort> = tools_factory.catalog_port();
        let tool_execution: Arc<dyn tools::ToolExecutionPort> = tools_factory.execution();
        let tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort> =
            tools_factory.binding();
        let reflection_history: Arc<dyn ReflectionHistoryStore> = Arc::new(FakeReflectionHistory);
        let task_access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
        let hook_runner: Arc<dyn HookPort> = Arc::new(FakeHook);

        struct NoopRunner;
        #[async_trait::async_trait]
        impl tools::AgentRunner for NoopRunner {
            async fn run_agent(
                &self,
                _request: tools::AgentRunRequest<'_>,
            ) -> tools::AgentRunTerminal {
                tools::AgentRunTerminal::Completed {
                    result: String::new(),
                }
            }
            async fn complete(
                &self,
                _prompt: &str,
                _system: &str,
                _cancellation: Arc<dyn tools::CancellationSignal>,
            ) -> String {
                String::new()
            }
        }
        let agent_runner: Arc<dyn tools::AgentRunner> = Arc::new(NoopRunner);
        let tool_result_materializer = crate::application::testing::test_tool_result_materializer();
        let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
        let config_query = wiring.config_query();
        let config_writer = wiring.config_writer();
        let session_management = wiring.session_management();
        let cwd = root.clone();

        // #1248 Task 3: Build RuntimeContextFactory for test shell.
        let runtime_context_factory = Arc::new(
            crate::application::runtime_context_factory::RuntimeContextFactory::new(
                tool_catalog.clone(),
                tool_execution.clone(),
                tool_context_binding.clone(),
                policy.clone(),
                reflection_history.clone(),
                task_access.clone(),
                hook_runner.clone(),
            ),
        );

        MainSessionShell {
            session_id: "test-session".to_string(),
            cwd,
            workspace,
            wiring: wiring.clone(),
            config_query,
            config_writer,
            session_management,
            provider_factory: Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            resolved_model: snapshot
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
            current_binding: Arc::new(std::sync::RwLock::new(binding.clone())),
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            system_blocks: Vec::new(),
            system_prompt_text: String::new(),
            initial_git_context: String::new(),
            user_context: String::new(),
            skill_catalog: tools::composition::wire_skills().catalog(),
            initial_skill_snapshot: tools::SkillCatalogSnapshot::from_descriptors(Vec::new()),
            memory_config: share::config::MemoryConfig::default(),
            context_size: 200_000,
            language: "en".to_string(),
            allow_all: true,
            verbose: false,
            resume: None,
            startup_resume: None,
            agent_runner,
            parent_context_source,
            tool_result_materializer,
            active_run,
            interaction_bridge: Arc::new(crate::application::interaction::InteractionBridge::new()),
            event_sink_factory: Arc::new(|tx| {
                crate::application::main_loop::ChatEventSinkHandle::new(
                    crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
                )
            }),
            input_port_factory: Arc::new(|queue, events| {
                crate::application::client::accessors::InputPortPair {
                    queue: crate::adapters::input_buffer::RuntimeQueueDrainPort::new(queue),
                    input_events: crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(
                        events,
                    ),
                }
            }),
            session_reminders: Arc::new(std::sync::RwLock::new(
                share::memory::SessionReminders::new(),
            )),
            runtime_context_factory,
        }
    }

    // ── Task 4 L1: Shell classification tests ──

    /// After bootstrap, MainSessionShell holds session-level state: wiring, workspace,
    /// session identity, prompt bootstrap, model switch. It does NOT hold a per-Run
    /// RuntimeContext instance.
    #[tokio::test(flavor = "current_thread")]
    async fn main_session_shell_holds_wiring_and_workspace_not_runtime_context() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;

        // Shell has wiring + workspace (session-level).
        let _wiring = &shell.wiring;
        let _workspace = &shell.workspace;
        let _session_id = &shell.session_id;

        // Shell has prompt bootstrap fields.
        let _system_blocks = &shell.system_blocks;
        let _initial_skill_snapshot = &shell.initial_skill_snapshot;
        let _initial_git = &shell.initial_git_context;
        // Shell has model switch fields.
        let _resolved_model = &shell.resolved_model;
        let _binding = shell.current_binding.read().unwrap().clone();

        // Shell does NOT have a RuntimeContext — it must be assembled per run.
        // (Compile-time check: MainSessionShell has no `runtime_context: RuntimeContext` field.)
    }

    /// Two assembler calls from the same shell produce different cancellation scopes
    /// but share Arc'd parent resources.
    #[tokio::test(flavor = "current_thread")]
    async fn two_assembler_calls_produce_different_cancel_shared_arcs() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;
        let config = RunConfigSnapshot::capture(shell.wiring.committed_config());

        let spec = RunSpec::main();
        let reasoning = Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium));
        let memory: Arc<dyn MemoryPort> = Arc::new(memory::NoOpMemory);
        let context: Arc<dyn ContextPort> = Arc::new(FakeContextPort);
        let sink = test_sink();

        let make_bindings = |context: Arc<dyn ContextPort>, memory: Arc<dyn MemoryPort>| {
            let binding = shell.current_binding.read().unwrap().clone();
            crate::application::runtime_context::RunContextBindings {
                context,
                provider: binding,
                interaction: shell.interaction_bridge.clone(),
                memory,
                config: config.clone(),
                cancel: crate::application::runtime_context::RunCancellationScope::new(),
                event_sink: sink.clone(),
                usage: crate::application::runtime_context::RunUsageTracker::new(),
                input: crate::application::runtime_context::RunInputBufferHandle::new(),
                reasoning: reasoning.clone(),
                tool_catalog: None,
            }
        };

        let ctx1 = shell
            .runtime_context_factory
            .assemble(&spec, make_bindings(context.clone(), memory.clone()), None)
            .expect("first assembly");

        let ctx2 = shell
            .runtime_context_factory
            .assemble(&spec, make_bindings(context.clone(), memory.clone()), None)
            .expect("second assembly");

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

    /// Model switch (updating `current_binding`) only affects the NEXT assembler call.
    /// An already-assembled RuntimeContext keeps its frozen binding.
    #[tokio::test(flavor = "current_thread")]
    async fn model_switch_affects_only_next_assembler() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;
        let config = RunConfigSnapshot::capture(shell.wiring.committed_config());

        let spec = RunSpec::main();
        let reasoning = Arc::new(std::sync::Mutex::new(provider::ReasoningLevel::Medium));
        let memory: Arc<dyn MemoryPort> = Arc::new(memory::NoOpMemory);
        let context: Arc<dyn ContextPort> = Arc::new(FakeContextPort);
        let sink = test_sink();

        let make_bindings = |context: Arc<dyn ContextPort>, memory: Arc<dyn MemoryPort>| {
            let binding = shell.current_binding.read().unwrap().clone();
            crate::application::runtime_context::RunContextBindings {
                context,
                provider: binding,
                interaction: shell.interaction_bridge.clone(),
                memory,
                config: config.clone(),
                cancel: crate::application::runtime_context::RunCancellationScope::new(),
                event_sink: sink.clone(),
                usage: crate::application::runtime_context::RunUsageTracker::new(),
                input: crate::application::runtime_context::RunInputBufferHandle::new(),
                reasoning: reasoning.clone(),
                tool_catalog: None,
            }
        };

        // First assembly captures the current binding.
        let ctx_before = shell
            .runtime_context_factory
            .assemble(&spec, make_bindings(context.clone(), memory.clone()), None)
            .expect("first assembly");

        let binding_before = ctx_before.provider();

        // Simulate model switch: create a new binding.
        let new_binding = crate::application::testing::test_binding(vec!["new model response"]);
        *shell.current_binding.write().unwrap() = new_binding.clone();

        // Second assembly picks up the new binding.
        let ctx_after = shell
            .runtime_context_factory
            .assemble(&spec, make_bindings(context.clone(), memory.clone()), None)
            .expect("second assembly");

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
            ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError>
            {
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
        let tool_result_materializer = crate::application::testing::test_tool_result_materializer();
        let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
        let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
            hook::build_dispatcher(
                &share::config::hooks::HooksConfig::default(),
                std::collections::HashMap::new(),
            )
            .expect("test hook dispatcher"),
        );
        let dependencies = RuntimeBootstrapDependencies::new(
            RuntimeCoreDependencies::new(
                workspace,
                wiring,
                Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
                Arc::new(TestReflectionHistory),
                policy.clone(),
                task_wiring.access(),
                Arc::new(context::test_support::UnavailableSessionManagement),
                hook_runner.clone(),
            ),
            RuntimeToolAssemblyDependencies::new(
                tools.catalog_port(),
                tools.execution(),
                tools.binding(),
                skill_wiring.catalog(),
                skill_wiring.loader(),
                tool_result_materializer,
                active_run,
            ),
            {
                Arc::new(
                    crate::application::runtime_context_factory::RuntimeContextFactory::new(
                        tools.catalog_port(),
                        tools.execution(),
                        tools.binding(),
                        policy,
                        Arc::new(TestReflectionHistory),
                        task_wiring.access(),
                        hook_runner.clone(),
                    ),
                )
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

    /// #1385 Task 7: `tui_launch_context()` reads binding from
    /// `shell.current_binding` — the single model binding lock.
    #[tokio::test(flavor = "current_thread")]
    async fn tui_launch_context_binding_comes_from_shell_lock() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;

        // Write a distinct binding into shell.current_binding.
        let switched = crate::application::testing::test_binding(vec!["tui sees this"]);
        *shell.current_binding.write().unwrap() = switched.clone();

        let handle = RuntimeHandle {
            shell: shell.clone(),
        };
        let client = AgentClientImpl {
            inner: Arc::new(handle),
        };

        let launch = client.tui_launch_context();
        assert!(
            Arc::ptr_eq(&launch.binding, &switched),
            "tui_launch_context must read binding from shell.current_binding"
        );
    }

    /// #1385 Task 7: accessors return values from `shell`, the single source.
    #[tokio::test(flavor = "current_thread")]
    async fn accessors_read_from_shell_single_source() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;

        let handle = RuntimeHandle {
            shell: shell.clone(),
        };
        let client = AgentClientImpl {
            inner: Arc::new(handle),
        };

        // All accessors must return shell values.
        assert_eq!(
            client.session_id(),
            shell.session_id,
            "session_id() must return shell.session_id"
        );
        assert_eq!(client.cwd(), &*shell.cwd, "cwd() must return shell.cwd");
        assert_eq!(
            client.resolved_model().model.id,
            shell.resolved_model.model.id,
            "resolved_model() must return shell.resolved_model"
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
        assert_eq!(
            client.shell().session_id,
            shell.session_id,
            "shell() accessor must return the actual MainSessionShell"
        );
        // #1385 Task 7: verbose migrated from ChatRuntimeContext to shell.
        assert_eq!(client.shell().verbose, shell.verbose);
    }

    /// #1385 Task 7: `shell.interaction_bridge` is the single source.
    /// `reply_interaction` and `cancel_interaction` both use it.
    #[tokio::test(flavor = "current_thread")]
    async fn interaction_bridge_is_single_source_on_shell() {
        let shell =
            make_test_shell(crate::application::runtime_context::ParentRunContextSource::new())
                .await;
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
}

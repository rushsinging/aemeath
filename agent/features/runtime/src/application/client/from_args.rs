use std::sync::Arc;
use std::time::Duration;

use sdk::SdkError;

use crate::application::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::application::startup::ChatBootstrapArgs;
use crate::application::startup::{
    build_agent_runner, build_hook_runner, resolve_concurrency_limits,
    resolve_model_runtime_settings, spawn_mcp_connect,
};
use crate::ports::legacy::ChatRuntimeContext;
use crate::ports::{ProviderBuildSpec, ProviderFactory, RequestSystemBlock};

use super::{AgentClientImpl, RuntimeHandle};

/// Runtime bootstrap 所需的活依赖；由 Composition 一次性构造并注入。
pub struct RuntimeBootstrapDependencies {
    workspace: project::WorkspaceViews,
    wiring: Arc<context::MainSessionWiring>,
    provider_factory: Arc<dyn ProviderFactory>,
    _tool_gateway: Arc<dyn tools::ToolCatalogGateway>,
    reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    policy: Arc<dyn policy::PolicyPort>,
    task_access: Arc<dyn task::TaskAccess>,
}

impl RuntimeBootstrapDependencies {
    pub fn new(
        workspace: project::WorkspaceViews,
        wiring: Arc<context::MainSessionWiring>,
        provider_factory: Arc<dyn ProviderFactory>,
        _tool_gateway: Arc<dyn tools::ToolCatalogGateway>,
        reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
        policy: Arc<dyn policy::PolicyPort>,
        task_access: Arc<dyn task::TaskAccess>,
    ) -> Self {
        Self {
            workspace,
            wiring,
            provider_factory,
            _tool_gateway,
            reflection_history,
            policy,
            task_access,
        }
    }

    pub fn reflection_history(&self) -> Arc<dyn memory::api::ReflectionHistoryStore> {
        self.reflection_history.clone()
    }

    pub fn task_access(&self) -> Arc<dyn task::TaskAccess> {
        self.task_access.clone()
    }

    pub fn wiring(&self) -> Arc<context::MainSessionWiring> {
        self.wiring.clone()
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
        _tool_gateway: _,
        reflection_history,
        policy,
        task_access,
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

    // 3. Session — startup resume FIRST, so the committed config snapshot read
    //    below reflects the target project's config (model, API key, hooks,
    //    memory, etc.) after a cross-project resume. For non-resume,
    //    committed_config() is unchanged so behavior is identical.
    let session_id = if let Some(resume_id) = args.resume.as_ref() {
        match crate::application::client::resume_helper::resume_session_to_backing(
            resume_id, &wiring,
        )
        .await
        {
            Ok(projection) => {
                log::info!(target: crate::LOG_TARGET, "startup resume: {}", projection.session_id);
                projection.session_id
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
        session_id
    };
    // Session id determined above; committed_config read below reflects
    // the target project after any cross-project resume.

    // 4. Read committed config AFTER any startup resume so the snapshot
    //    reflects the target project. For non-resume this is identical to
    //    reading before — committed_config() is unchanged.
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
    };
    let binding = provider_factory
        .build(spec)
        .map_err(|error| SdkError::Init(error.to_string()))?;

    // 11. Tooling
    // MemoryPortSource: delegates to wiring.committed_memory() at execution
    // time so resume swaps are transparent to the already-registered tool.
    let memory_source: Arc<dyn tools::MemoryPortSource> = {
        struct WiringMemoryPortSource {
            wiring: Arc<context::MainSessionWiring>,
        }
        impl tools::MemoryPortSource for WiringMemoryPortSource {
            fn current(&self) -> Arc<dyn memory::MemoryPort> {
                self.wiring.committed_memory()
            }
        }
        Arc::new(WiringMemoryPortSource {
            wiring: wiring.clone(),
        })
    };
    let tool_wiring = tools::composition::wire_builtin_catalog_execution(
        task_access.clone(),
        memory_source,
        workspace.control(),
    )
    .map_err(|error| SdkError::Init(error.to_string()))?;
    let tool_catalog = tool_wiring.catalog();
    let tool_execution = tool_wiring.execution();
    let tool_context_binding = tool_wiring.binding();
    let available_tools = tool_catalog
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .map_err(|error| SdkError::Init(error.to_string()))?
        .tools
        .iter()
        .map(|descriptor| descriptor.name.as_str().to_string())
        .collect();
    let skill_wiring = tools::composition::wire_skills();
    let skill_query =
        tools::SkillQuery::new(cwd.clone(), snapshot.skills().dirs.clone(), available_tools);
    let descriptors = skill_wiring.catalog().list(skill_query.clone());
    let materialized = skill_wiring
        .materializer()
        .materialize_available(tools::SkillMaterializationQuery::new(
            skill_query.project_root,
            skill_query.extra_dirs,
            skill_query.available_tools,
        ))
        .await
        .map_err(|error| SdkError::Init(error.to_string()))?;
    let fragments = materialized
        .fragments()
        .iter()
        .map(|fragment| (fragment.stable_key(), fragment))
        .collect::<std::collections::HashMap<_, _>>();
    let skills_map = descriptors
        .into_iter()
        .filter_map(|descriptor| {
            let fragment = fragments.get(descriptor.name())?;
            Some((
                descriptor.name().to_string(),
                sdk::SkillView {
                    name: descriptor.name().to_string(),
                    aliases: descriptor.aliases().to_vec(),
                    description: Some(descriptor.description().to_string()),
                    content: fragment.content().to_string(),
                    source: Some(descriptor.source().path.clone()),
                },
            ))
        })
        .collect();
    let mcp_manager = spawn_mcp_connect(&tool_wiring, &cwd).await;

    // 12. Hook runner
    let hook_runner = build_hook_runner(Some(snapshot.hooks()), &cwd);
    let _hook_runner_before = hook_runner.clone();

    // 13. Tool Result blob 与 materialization policy。
    let blob_adapter = Arc::new(
        storage::FileSystemBlobAdapter::new(share::config::paths::global_agents_dir())
            .map_err(|error| SdkError::Init(error.to_string()))?,
    );
    let blob_store = Arc::new(
        crate::adapters::tool_result_blob::AtomicBlobToolResultStore::new(
            blob_adapter,
            share::config::paths::global_agents_dir(),
        ),
    );
    let tool_result_policy = snapshot.tool_result_policy();
    let tool_result_materializer = Arc::new(
        crate::application::tool_result_materialization::ToolResultMaterializer::new(
            blob_store,
            crate::application::tool_result_materialization::ToolResultMaterializationPolicy::new(
                tool_result_policy.threshold_chars(),
                tool_result_policy.preview_head_chars(),
                tool_result_policy.preview_tail_chars(),
            ),
        ),
    );

    // 14. Agent runner 与 Main/Sub 共享同一个 per-Run registry 和 materializer。
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());

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

    // 17. Memory port — gate-aware, from wiring.
    let memory: Arc<dyn memory::api::MemoryPort> = wiring.committed_memory();

    let agent_runner = build_agent_runner(
        Some(snapshot.models()),
        Some(snapshot.agents()),
        provider_factory.clone(),
        snapshot.api_timeout_secs(),
        hook_runner.clone(),
        runtime_settings.reasoning,
        active_run.clone(),
        policy.clone(),
        max_tool_concurrency,
        agent_semaphore.clone(),
        tool_result_materializer.clone(),
        workspace.clone(),
        tool_catalog.clone(),
        tool_execution.clone(),
        tool_context_binding.clone(),
    );

    // 18. Prompt bundle
    let prompt_context = PromptContext::new(
        &cwd,
        Some(&binding.model.provider),
        Some(&binding.model.model),
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
    let system_blocks = vec![
        RequestSystemBlock::Cacheable(static_prompt),
        RequestSystemBlock::Text(prompt_parts.dynamic_part),
    ];
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

    // 20. context_size / verbose 合并
    let context_size =
        snapshot.resolve_context_size(Some(args.context_size), resolved_model.model.context_window);

    // 20. 组装 context
    let memory_config = snapshot.memory().clone();
    let context = ChatRuntimeContext {
        resources: crate::application::resources::RuntimeResources {
            binding: Arc::new(binding.clone()),
            provider_factory: provider_factory.clone(),
            tool_catalog,
            tool_execution,
            tool_context_binding,
            system_blocks,
            system_prompt_text,
            user_context: prompt_parts.claude_md,
            agent_runner,
            tool_result_materializer,
            task_access,
            skills_map,
            hook_runner,
            memory_config,
            agent_semaphore,
            memory,
            reflection_history,
            policy,
            allow_all: args.allow_all,
            context_size,
            language: snapshot.language().to_string(),
        },
        verbose: args.verbose,
        resume: args.resume,
    };

    // 21. 构建 handle
    let handle = RuntimeHandle {
        context,
        cwd,
        resolved_model,
        session_id,
        max_tool_concurrency,
        max_agent_concurrency,
        _mcp_manager: mcp_manager,
        current_binding: std::sync::RwLock::new(Arc::new(binding)),
        active_run,
        workspace,
        wiring: wiring.clone(),
        config_query,
        config_writer,
        event_sink_factory: Arc::new(|tx| {
            crate::application::chat::ChatEventSinkHandle::new(
                crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
            )
        }),
        session_reminders: Arc::new(std::sync::RwLock::new(
            share::memory::SessionReminders::new(),
        )),
    };

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
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    #[tokio::test(flavor = "current_thread")]
    async fn from_args_preserves_workspace_views_and_main_policy_identity() {
        struct TestReflectionHistory;

        #[async_trait::async_trait]
        impl memory::api::ReflectionHistoryQuery for TestReflectionHistory {
            async fn list(
                &self,
                _limit: usize,
            ) -> Result<Vec<memory::api::ReflectionRecord>, memory::api::MemoryError> {
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
        let config = config::wire_project_config(&root)
            .await
            .expect("wire config");
        let task_wiring = task::wire_task();
        let wiring = context::test_support::wire_in_memory(
            &workspace,
            task_wiring.persist(),
            config.reader(),
            config.participant(),
            Arc::new(context::ProductionMainContextFactory::new(Arc::new(
                context::NoOpCanonicalSessionWriter,
            ))),
        )
        .await;
        let policy: Arc<dyn policy::PolicyPort> = Arc::new(policy::AllowAllPolicy);
        let dependencies = RuntimeBootstrapDependencies::new(
            workspace,
            wiring,
            Arc::new(crate::ports::provider_port::fake::FakeProviderFactory),
            tools::wire_tools(),
            Arc::new(TestReflectionHistory),
            policy.clone(),
            Arc::new(task::TaskStore::new()),
        );
        let client = from_args_with_workspace(args, dependencies)
            .await
            .expect("build client with workspace");

        assert!(
            Arc::ptr_eq(&client.inner.context.resources.policy, &policy),
            "Main Run 必须保留 Composition 注入的同一 PolicyPort 实例"
        );
        assert_eq!(
            client.inner.workspace.read().current_path_base(),
            sub.canonicalize().expect("canonicalize subdirectory")
        );

        original
            .control()
            .change_directory(root.clone())
            .expect("change original clone back to root");
        assert_eq!(
            client.inner.workspace.read().current_path_base(),
            root.canonicalize().expect("canonicalize root")
        );
    }

    /// Structural guard (H3): startup resume must happen BEFORE the committed
    /// config snapshot is read, so the snapshot reflects the target project
    /// after a cross-project resume.
    ///
    /// The ordering invariant: `resume_session_to_backing` (the resume block)
    /// must appear textually before `committed_config()` in
    /// `from_args_with_workspace`.
    #[test]
    fn startup_resume_precedes_committed_config_read() {
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
            "resume_session_to_backing (post-resume) should precede the committed_config snapshot read — \
           H3: snapshot must be determined after startup resume so model/API key/MemoryConfig \
           come from the target project"
        );
    }
}

use std::sync::{Arc, Mutex};

use sdk::SdkError;

use crate::adapters::runtime::LlmClientAdapter;
use crate::application::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::application::startup::{
    self as bootstrap, build_agent_runner, build_hook_runner, resolve_api_key, resolve_base_url,
    resolve_concurrency_limits, resolve_model_runtime_settings, spawn_mcp_connect,
};
use crate::application::startup::{start_session, ChatBootstrapArgs};
use crate::ports::legacy::ChatRuntimeContext;
use crate::ports::legacy::ProviderInfoPort;
use context::skill::{load_all_skills, Skill};
use provider::SystemBlock;
use storage::TaskStore;

use super::{AgentClientImpl, RuntimeHandle};
use crate::LOG_TARGET;

/// Runtime bootstrap 所需的活依赖；由 Composition 一次性构造并注入。
pub struct RuntimeBootstrapDependencies {
    workspace: project::WorkspaceViews,
    config_reader: Arc<dyn config::ConfigReader>,
    config_query: Arc<dyn config::ConfigQuery>,
    config_writer: Arc<dyn config::ConfigWriter>,
    memory: Arc<dyn memory::MemoryPort>,
    reflection_history: Arc<dyn memory::ReflectionHistoryStore>,
    provider_gateway: Arc<dyn provider::LlmProviderGateway>,
    tool_gateway: Arc<dyn tools::ToolCatalogGateway>,
    policy: Arc<dyn policy::PolicyPort>,
    task_access: Arc<dyn task::TaskAccess>,
    session_tasks: Arc<dyn context::LegacyTaskCapture>,
}

impl RuntimeBootstrapDependencies {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: project::WorkspaceViews,
        config: RuntimeConfigDependencies,
        memory: Arc<dyn memory::MemoryPort>,
        reflection_history: Arc<dyn memory::ReflectionHistoryStore>,
        provider_gateway: Arc<dyn provider::LlmProviderGateway>,
        tool_gateway: Arc<dyn tools::ToolCatalogGateway>,
        policy: Arc<dyn policy::PolicyPort>,
        task_access: Arc<dyn task::TaskAccess>,
        session_tasks: Arc<dyn context::LegacyTaskCapture>,
    ) -> Self {
        Self {
            workspace,
            config_reader: config.reader,
            config_query: config.query,
            config_writer: config.writer,
            memory,
            reflection_history,
            provider_gateway,
            tool_gateway,
            policy,
            task_access,
            session_tasks,
        }
    }

    pub fn reflection_history(&self) -> Arc<dyn memory::ReflectionHistoryStore> {
        self.reflection_history.clone()
    }

    pub fn task_access(&self) -> Arc<dyn task::TaskAccess> {
        self.task_access.clone()
    }

    pub fn session_tasks(&self) -> Arc<dyn context::LegacyTaskCapture> {
        self.session_tasks.clone()
    }
}

pub struct RuntimeConfigDependencies {
    reader: Arc<dyn config::ConfigReader>,
    query: Arc<dyn config::ConfigQuery>,
    writer: Arc<dyn config::ConfigWriter>,
}

impl RuntimeConfigDependencies {
    pub fn new(
        reader: Arc<dyn config::ConfigReader>,
        query: Arc<dyn config::ConfigQuery>,
        writer: Arc<dyn config::ConfigWriter>,
    ) -> Self {
        Self {
            reader,
            query,
            writer,
        }
    }
}

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
///
/// `task_access` 和 `session_tasks` 由 Composition 层注入：Runtime 不得自行创建
/// Task BC 的 backing 或持久化封套（跨域越权，#890）。
pub async fn from_args_with_workspace(
    args: ChatBootstrapArgs,
    dependencies: RuntimeBootstrapDependencies,
) -> Result<AgentClientImpl, SdkError> {
    let RuntimeBootstrapDependencies {
        workspace,
        config_reader,
        config_query,
        config_writer,
        memory,
        reflection_history,
        provider_gateway,
        tool_gateway,
        policy,
        task_access,
        session_tasks,
    } = dependencies;
    // 1. Guidance 目录初始化
    context::guidance::init_guidance_dir();

    // 2. 解析 cwd
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 3. 使用 Composition 已加载的唯一 committed 配置。
    let snapshot = config_reader.committed_snapshot();

    // 4. 日志已由 Composition 在进入 Runtime 前初始化。

    // 5. 权限模式

    // 6. 模型选择与运行参数解析 — 由 ConfigSnapshot 收敛 config 语义。
    let runtime_model = snapshot
        .resolve_runtime_model(args.model.as_deref(), args.max_tokens)
        .map_err(|e| SdkError::Init(e.to_string()))?;
    let resolved_model = runtime_model.resolved_model().clone();
    let driver = resolved_model.driver.as_str();
    // 7. API key
    let api_key = resolve_api_key(&resolved_model).ok_or_else(|| {
        SdkError::Init(
            "API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json".to_string(),
        )
    })?;

    // 8. Base URL + model + runtime settings
    let base_url = resolve_base_url(args.base_url.clone(), &resolved_model);
    let model = resolved_model.model.id.clone();
    let runtime_settings = resolve_model_runtime_settings(
        runtime_model.max_tokens(),
        &resolved_model.model,
        !args.no_think,
    );

    log::info!(target: LOG_TARGET,
        "[main] source={} api={} model={} reasoning={} args.no_think={}",
        resolved_model.source_key,
        driver,
        model,
        runtime_settings.reasoning,
        args.no_think
    );

    // 9. LLM client
    let client = Arc::new(
        bootstrap::build_llm_client_with_gateway(
            provider_gateway.as_ref(),
            driver,
            api_key,
            base_url,
            model.clone(),
            &resolved_model,
            &runtime_settings,
            args.max_reasoning.as_deref(),
            snapshot.api_timeout_secs(),
        )
        .map_err(|error| SdkError::Init(error.to_string()))?,
    );

    // 10. Tooling
    // Legacy store 仅供持久化兼容（snapshot/restore、input_gate clear，#891）。
    let task_store = Arc::new(TaskStore::new());
    let skills_map = load_configured_skills(&cwd, Some(snapshot.skills()));
    if !skills_map.is_empty() {
        log::info!(target: LOG_TARGET, "[Skills] loaded {} skills", skills_map.len());
    }
    let skills = Arc::new(tokio::sync::Mutex::new(skills_map.clone()));
    let registry = {
        let reg = tool_gateway.new_registry();
        tool_gateway.register_all_tools(
            &reg,
            task_access.clone(),
            skills.clone(),
            workspace.control(),
        );
        Arc::new(reg)
    };
    let mcp_manager = spawn_mcp_connect(registry.clone(), &cwd).await;

    // 11. Hook runner
    let hook_runner = build_hook_runner(Some(snapshot.hooks()), &cwd);
    let _hook_runner_before = hook_runner.clone();

    // 12. Session
    let session_id = start_session(args.resume.clone());

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

    // 14. Runtime owns concurrency and shares the same agent semaphore across Main/Sub runs.
    let (max_tool_concurrency, max_agent_concurrency) = resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        &snapshot,
    );
    let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
    let agent_runner = build_agent_runner(
        Some(snapshot.models()),
        Some(snapshot.agents()),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
        snapshot.api_timeout_secs(),
        active_run.clone(),
        policy.clone(),
        max_tool_concurrency,
        agent_semaphore.clone(),
        tool_result_materializer.clone(),
        workspace.clone(),
    );

    // 15. Prompt bundle
    let client_adapter = LlmClientAdapter::new(client.clone());
    let prompt_context = PromptContext::new(
        &cwd,
        Some(client_adapter.provider_name()),
        Some(client_adapter.model_name()),
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
        &skills,
    )
    .await;
    let system_blocks = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];
    let system_prompt_text: String = system_blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    // 16. Concurrency
    log::info!(target: LOG_TARGET,
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 17. context_size / verbose 合并
    let context_size =
        snapshot.resolve_context_size(Some(args.context_size), resolved_model.model.context_window);

    // 18. 组装 context
    let memory_config = snapshot.memory().clone();
    let context = ChatRuntimeContext {
        resources: crate::application::resources::RuntimeResources {
            client,
            registry,
            system_blocks,
            system_prompt_text,
            user_context: prompt_parts.claude_md,
            agent_runner,
            tool_result_materializer,
            task_store,
            task_access,
            skills_map,
            hook_runner,
            memory_config,
            memory,
            reflection_history,
            policy,
            agent_semaphore,
            allow_all: args.allow_all,
            context_size,
            language: snapshot.language().to_string(),
        },
        verbose: args.verbose,
        resume: args.resume,
    };

    // 19. 构建 handle
    let current_client = context.resources.client.clone();
    let handle = RuntimeHandle {
        context,
        cwd,
        resolved_model,
        session_id,
        session_tasks,
        max_tool_concurrency,
        max_agent_concurrency,
        _mcp_manager: mcp_manager,
        current_client: std::sync::RwLock::new(current_client),
        active_run,
        current_chain: Arc::new(Mutex::new(context::session::ChatChain::default())),
        frozen_chats: Arc::new(Mutex::new(Vec::new())),
        active_summary: Arc::new(Mutex::new(None)),
        workspace,
        config_reader,
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

fn load_configured_skills(
    cwd: &std::path::Path,
    skills_config: Option<&share::config::SkillsConfig>,
) -> std::collections::HashMap<String, Skill> {
    let dirs = skills_config.map(|c| c.dirs.clone()).unwrap_or_default();
    load_all_skills(cwd, &dirs)
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
    async fn from_args_keeps_cloned_workspace_views_synchronized() {
        // Capture-only fake for tests — no-op that never reaches the real Task BC.
        struct NoOpTaskCapture;
        impl context::LegacyTaskCapture for NoOpTaskCapture {
            fn capture_legacy_session(
                &self,
                _session: &mut context::session::Session,
            ) -> Result<(), String> {
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
        let dependencies = RuntimeBootstrapDependencies::new(
            workspace,
            RuntimeConfigDependencies::new(config.reader(), config.query(), config.writer()),
            Arc::new(memory::NoOpMemory),
            Arc::new(memory::AtomicDatasetReflectionHistoryStore::new(
                Arc::new(storage::FileSystemDatasetAdapter::new(temp.path()).unwrap()),
                memory::ProjectMemoryKey::derive(root.to_str().unwrap(), None).unwrap(),
            )),
            provider::wire_provider(),
            tools::wire_tools(),
            Arc::new(policy::AllowAllPolicy),
            Arc::new(task::TaskStore::new()),
            Arc::new(NoOpTaskCapture),
        );
        let client = from_args_with_workspace(args, dependencies)
            .await
            .expect("build client with workspace");

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
}

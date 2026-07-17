use std::sync::{Arc, Mutex};

use sdk::SdkError;

use crate::adapters::runtime::LlmClientAdapter;
use crate::application::config_app_service::ConfigAppService;
use crate::application::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::application::startup::{
    self as bootstrap, apply_config_permission_mode, build_agent_runner, build_hook_runner,
    init_logging, resolve_api_key, resolve_base_url, resolve_concurrency_limits,
    resolve_model_runtime_settings, spawn_mcp_connect,
};
use crate::application::startup::{set_session_id, start_session, ChatBootstrapArgs};
use crate::ports::config::ConfigReader;
use crate::ports::legacy::ChatRuntimeContext;
use crate::ports::legacy::ProviderInfoPort;
use context::skill::{load_all_skills, Skill};
use provider::ProviderDriverKind;
use provider::SystemBlock;
use storage::TaskStore;
use tools::api as tools_crate;
use tools::api::ToolRegistry;

use super::{AgentClientImpl, RuntimeHandle};
use crate::LOG_TARGET;

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
pub async fn from_args(mut args: ChatBootstrapArgs) -> Result<AgentClientImpl, SdkError> {
    // 1. Guidance 目录初始化
    context::guidance::init_guidance_dir();

    // 2. 解析 cwd
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 3. 加载配置
    let svc = ConfigAppService::new(Some(&cwd));
    svc.load().await.ok();
    let snapshot = svc.snapshot().await;

    // 4. 日志初始化
    init_logging(snapshot.logging());

    // 5. 权限模式
    apply_config_permission_mode(&mut args, snapshot.allow_all());

    // 6. 模型选择与运行参数解析 — 由 ConfigSnapshot 收敛 config 语义。
    let runtime_model = snapshot
        .resolve_runtime_model(args.model.as_deref(), args.max_tokens)
        .map_err(|e| SdkError::Init(e.to_string()))?;
    let resolved_model = runtime_model.resolved_model().clone();
    let driver =
        ProviderDriverKind::parse(&resolved_model.driver).unwrap_or(ProviderDriverKind::OpenAI);
    // 7. API key
    let api_key = resolve_api_key(args.api_key.take(), &resolved_model, None).ok_or_else(|| {
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
        driver.as_str(),
        model,
        runtime_settings.reasoning,
        args.no_think
    );

    // 9. LLM client
    let client = Arc::new(bootstrap::build_llm_client(
        driver,
        api_key,
        base_url,
        model.clone(),
        &resolved_model,
        &runtime_settings,
        args.max_reasoning.as_deref(),
        snapshot.api_timeout_secs(),
    ));

    // 10. Tooling
    let task_store = Arc::new(TaskStore::new());
    let _task_store_before = task_store.clone();
    let skills_map = load_configured_skills(&cwd, Some(snapshot.skills()));
    if !skills_map.is_empty() {
        log::info!(target: LOG_TARGET, "[Skills] loaded {} skills", skills_map.len());
    }
    let skills = Arc::new(tokio::sync::Mutex::new(skills_map.clone()));
    let registry = {
        let reg = ToolRegistry::new();
        tools_crate::register_all_tools(&reg, task_store.clone(), skills.clone());
        Arc::new(reg)
    };
    let mcp_manager = spawn_mcp_connect(registry.clone(), &cwd).await;

    // 11. Hook runner
    let hook_runner = build_hook_runner(Some(snapshot.hooks()), &cwd);
    let _hook_runner_before = hook_runner.clone();

    // 12. Session
    let session_id = start_session(args.resume.clone());
    set_session_id(session_id.clone());

    // 13. Agent runner 与 Main/Sub 共享同一个 per-Run registry。
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
    let agent_runner = build_agent_runner(
        Some(snapshot.models()),
        Some(snapshot.agents()),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
        snapshot.api_timeout_secs(),
        active_run.clone(),
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
    let (max_tool_concurrency, max_agent_concurrency) = resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        snapshot.max_tool_concurrency(),
        snapshot.max_agent_concurrency(),
    );
    let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));
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
    let reasoning_graph_config = Some(workflow::GraphRuntimeConfig::from_shared(
        snapshot.reasoning_graph(),
    ));
    let context = ChatRuntimeContext {
        resources: crate::application::resources::RuntimeResources {
            client,
            registry,
            system_blocks,
            system_prompt_text,
            user_context: prompt_parts.claude_md,
            agent_runner,
            task_store,
            skills_map,
            hook_runner,
            memory_config,
            agent_semaphore,
            allow_all: args.allow_all,
            context_size,
            language: snapshot.language().to_string(),
            reasoning_graph_config,
        },
        verbose: args.verbose,
        resume: args.resume,
    };

    // 19. 构建 handle
    let current_client = context.resources.client.clone();
    let workspace = project::WorkspaceService::new(cwd.clone());
    let handle = RuntimeHandle {
        context,
        cwd,
        resolved_model,
        session_id,
        max_tool_concurrency,
        max_agent_concurrency,
        _mcp_manager: mcp_manager,
        current_client: std::sync::RwLock::new(current_client),
        active_run,
        current_chain: Arc::new(Mutex::new(context::session::ChatChain::default())),
        frozen_chats: Arc::new(Mutex::new(Vec::new())),
        active_summary: Arc::new(Mutex::new(None)),
        workspace,
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

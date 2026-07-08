use std::sync::{Arc, Mutex};

use sdk::{ChangeSet, SdkError};
use tokio::sync::watch;

use crate::business::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::core::config_app_service::ConfigAppService;
use crate::core::config_port::ConfigReader;
use crate::core::port::ChatRuntimeContext;
use crate::core::port::ProviderInfoPort;
use crate::utils::adapter::LlmClientAdapter;
use crate::utils::bootstrap::{
    self, apply_config_permission_mode, build_agent_runner, build_hook_runner, init_logging,
    resolve_api_key, resolve_base_url, resolve_concurrency_limits, resolve_model_runtime_settings,
    spawn_mcp_connect,
};
use crate::utils::bootstrap::{set_session_id, start_session, ChatBootstrapArgs};
use prompt::api::skill::{load_all_skills, Skill};
use provider::api::ProviderDriverKind;
use provider::api::SystemBlock;
use storage::api::TaskStore;
use tools::api as tools_crate;
use tools::api::ToolRegistry;

use super::{AgentClientImpl, RuntimeHandle};
use crate::LOG_TARGET;

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
pub async fn from_args(mut args: ChatBootstrapArgs) -> Result<AgentClientImpl, SdkError> {
    // 1. Guidance 目录初始化
    prompt::api::guidance::init_guidance_dir();

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

    // 6. 模型选择 — 直接使用 ModelsConfig::select_for_run
    let resolved_model = snapshot
        .models()
        .select_for_run(args.model.as_deref())
        .map_err(|e| SdkError::Init(e.to_string()))?;
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
        args.max_tokens,
        &resolved_model.model,
        Some(snapshot.max_tokens()),
        !args.no_think,
    )
    .map_err(|e| SdkError::Init(e.to_string()))?;

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
    ));

    // 10. Tooling
    let task_store = Arc::new(TaskStore::new());
    let task_store_before = task_store.clone();
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
    let hook_runner_before = hook_runner.clone();

    // 12. Session
    let session_id = start_session(args.resume.clone());
    set_session_id(session_id.clone());

    // 13. Agent runner
    let agent_runner = build_agent_runner(
        Some(snapshot.models()),
        Some(snapshot.agents()),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
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

    let static_prompt = crate::business::prompt::prompt_build_ext::build_static_prompt(
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
    let reasoning_graph_config = Some(
        crate::business::reasoning_graph::GraphRuntimeConfig::from_shared(
            snapshot.reasoning_graph(),
        ),
    );
    let context = ChatRuntimeContext {
        resources: crate::core::resources::RuntimeResources {
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
    let (change_tx, change_rx) = watch::channel(ChangeSet::empty());
    let current_client = context.resources.client.clone();
    let workspace = project::api::WorkspaceService::new(cwd.clone());
    let handle = RuntimeHandle {
        context,
        cwd,
        resolved_model,
        session_id,
        max_tool_concurrency,
        max_agent_concurrency,
        _mcp_manager: mcp_manager,
        current_client: std::sync::RwLock::new(current_client),
        current_cancel: Arc::new(Mutex::new(tokio_util::sync::CancellationToken::new())),
        current_chain: Arc::new(Mutex::new(crate::business::session::ChatChain::default())),
        frozen_chats: Arc::new(Mutex::new(Vec::new())),
        active_summary: Arc::new(Mutex::new(None)),
        workspace,
        change_tx,
        change_rx,
        hook_runner: Some(hook_runner_before.clone()),
        task_store: Some(task_store_before.clone()),
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

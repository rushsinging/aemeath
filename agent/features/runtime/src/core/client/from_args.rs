use std::sync::{Arc, Mutex};

use sdk::{ChangeSet, SdkError};
use tokio::sync::watch;

use crate::business::prompt::build::{build_system_prompt_parts, PromptContext};
use crate::core::config_app_service::ConfigAppService;
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
    let config_file = ConfigAppService::new(Some(&cwd)).load().await.ok();

    // 4. 日志初始化
    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&share::config::LoggingConfig::default()),
    );

    // 5. 权限模式
    apply_config_permission_mode(
        &mut args,
        config_file
            .as_ref()
            .map(|c| {
                matches!(
                    c.permissions.mode,
                    share::config::PermissionModeConfig::AllowAll
                )
            })
            .unwrap_or(false),
    );

    // 6. 模型选择 — 直接使用 ModelsConfig::select_for_run
    let config = config_file.as_ref().ok_or_else(|| {
        SdkError::Init(
            "未指定模型。请使用 --model <来源>/<模型>，或在 ~/.agents/aemeath.json 配置 models.default".to_string(),
        )
    })?;
    let resolved_model = config
        .models
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
        config_file.as_ref().map(|c| c.model.max_tokens),
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
    let skills_map = load_configured_skills(&cwd, config_file.as_ref().map(|c| &c.skills));
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
    let hook_runner = build_hook_runner(config_file.as_ref(), &cwd);
    let hook_runner_before = hook_runner.clone();

    // 12. Session
    let session_id = start_session(args.resume.clone());
    set_session_id(session_id.clone());

    // 13. Agent runner
    let agent_runner = build_agent_runner(
        config_file.as_ref(),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
    );

    // 15. Prompt bundle
    let prompt_memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let client_adapter = LlmClientAdapter::new(client.clone());
    let prompt_context = PromptContext::new(
        &cwd,
        Some(client_adapter.provider_name()),
        Some(client_adapter.model_name()),
    );
    let prompt_parts = build_system_prompt_parts(
        &prompt_context,
        &hook_runner,
        &prompt_memory_config,
        config_file
            .as_ref()
            .map(|c| c.language.as_str())
            .unwrap_or("en"),
    )
    .await;

    let static_prompt = crate::business::prompt::prompt_build_ext::build_static_prompt(
        &cwd,
        &model,
        runtime_settings.reasoning,
        config_file.as_ref(),
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
        config_file
            .as_ref()
            .map(|c| c.tools.max_concurrency)
            .unwrap_or(0),
        config_file
            .as_ref()
            .map(|c| c.agents.max_concurrency)
            .unwrap_or(0),
    );
    let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));
    log::info!(target: LOG_TARGET,
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 17. context_size / verbose 合并
    let snapshot_context_size = config_file
        .as_ref()
        .map(|c| c.model.context_size)
        .unwrap_or(0);
    let context_size = {
        let cli = args.context_size;
        let model_cw = resolved_model.model.context_window;
        if cli > 0 {
            cli
        } else if snapshot_context_size > 0 {
            snapshot_context_size
        } else if model_cw > 0 {
            model_cw
        } else {
            128_000
        }
    };

    // 18. 组装 context
    let memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let reasoning_graph_config = config_file.as_ref().map(|c| {
        crate::business::reasoning_graph::GraphRuntimeConfig::from_shared(&c.reasoning_graph)
    });
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
            language: config_file
                .as_ref()
                .map(|c| c.language.clone())
                .unwrap_or_else(|| "en".to_string()),
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
        current_messages: Arc::new(Mutex::new(Vec::new())),
        frozen_chats: Arc::new(Mutex::new(Vec::new())),
        active_summary: Arc::new(Mutex::new(None)),
        skip_first_pending_turn: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

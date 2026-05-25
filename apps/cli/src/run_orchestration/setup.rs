mod concurrency;
mod model_runtime;
mod permissions;
mod prompt_bundle;
mod provider_client;
mod runtime_support;
mod tooling;

use super::{chat_mode_selection, ChatModeSelection};
use crate::cli::Args;
use crate::logging_setup::init_logging;
use crate::model_selection::select_model_for_run;
use concurrency::resolve_concurrency_limits;
use kernel::config::models::ResolvedModel;
use kernel::mcp_manager::McpConnectionManager;
use model_runtime::{resolve_model_runtime_settings, ReasoningConfigInput};
use permissions::apply_config_permission_mode;
use prompt_bundle::build_chat_prompt_bundle;
use provider_client::{build_llm_client, resolve_api_key, resolve_base_url};
use runtime_support::{build_agent_runner, build_hook_runner, build_json_logger, start_session};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tooling::build_chat_tooling;

use crate::application::chat::ChatRuntimeContext;

pub(super) struct ChatBootstrap {
    pub args: Args,
    pub cwd: PathBuf,
    pub resolved_model: ResolvedModel,
    pub session_id: String,
    pub context: ChatRuntimeContext,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub mode_selection: ChatModeSelection,
    pub _mcp_manager: Arc<McpConnectionManager>,
}

pub(super) async fn bootstrap_chat(mut args: Args) -> ChatBootstrap {
    // 加载 config.json 以获取 provider 默认值 (apiKey, baseUrl, model)
    // 优先级: CLI args > env vars > 项目 config.json > 全局 config.json > built-in defaults

    // 初始化 guidance 目录（首次运行时生成默认 guidance 文件）
    kernel::guidance::init_guidance_dir();

    let cwd = args
        .cwd
        .clone()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let config_file = kernel::config::ConfigManager::new(Some(&cwd))
        .load()
        .await
        .ok();

    // 初始化日志系统（在 config 加载之后，使用配置中的日志级别）
    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&kernel::config::LoggingConfig::default()),
    );

    apply_config_permission_mode(&mut args, config_file.as_ref());

    let requested_model = args.model.as_deref();
    let resolved_model = select_model_for_run(requested_model, config_file.as_ref())
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let api_type = resolved_model.api;

    let api_key = resolve_api_key(args.api_key.take(), &resolved_model, None).unwrap_or_else(|| {
        eprintln!("Error: API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json");
        std::process::exit(1);
    });

    let base_url = resolve_base_url(args.base_url.clone(), &resolved_model);
    let model = resolved_model.model.id.clone();
    let runtime_settings = resolve_model_runtime_settings(
        args.max_tokens,
        &resolved_model.model,
        config_file.as_ref(),
        !args.no_think,
        ReasoningConfigInput {
            cli_reasoning_effort: args.reasoning_effort.clone(),
            env_reasoning_effort: std::env::var("AEMEATH_REASONING_EFFORT").ok(),
        },
    )
    .unwrap_or_else(|e| {
        log::error!("{}", e);
        std::process::exit(1);
    });

    log::info!(
        "[main] source={} api={} model={} reasoning={} effort={:?} args.no_think={}",
        resolved_model.source_key,
        api_type.as_str(),
        model,
        runtime_settings.reasoning,
        runtime_settings.reasoning_effort,
        args.no_think
    );

    let client = build_llm_client(
        api_type,
        api_key,
        base_url,
        model.clone(),
        &resolved_model,
        &runtime_settings,
    );

    let client = std::sync::Arc::new(client);

    let task_store = std::sync::Arc::new(kernel::task::TaskStore::new());
    let tooling = build_chat_tooling(
        &cwd,
        config_file.as_ref().map(|config| &config.skills),
        task_store.clone(),
    )
    .await;

    let hook_runner = build_hook_runner(config_file.as_ref(), &cwd);
    let session_id = start_session(args.resume.clone());

    let json_logger = build_json_logger(&session_id, config_file.as_ref());
    let agent_runner = build_agent_runner(
        config_file.as_ref(),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
        json_logger.clone(),
    );
    let prompt_memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let prompt_bundle = build_chat_prompt_bundle(
        &cwd,
        &model,
        runtime_settings.reasoning,
        config_file.as_ref(),
        &hook_runner,
        prompt_memory_config,
        &tooling.skills,
        client.provider_name(),
        client.model_name(),
    )
    .await;

    let (max_tool_concurrency, max_agent_concurrency) = resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        config_file.as_ref(),
    );
    let agent_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));

    log::info!(
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    let memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let mode_selection = chat_mode_selection(&args);
    let context = ChatRuntimeContext {
        client,
        registry: tooling.registry,
        system_blocks: prompt_bundle.system_blocks,
        system_prompt_text: prompt_bundle.system_prompt_text,
        user_context: prompt_bundle.user_context,
        agent_runner,
        task_store,
        skills_map: tooling.skills_map,
        hook_runner,
        memory_config,
        json_logger,
        agent_semaphore,
    };

    ChatBootstrap {
        args,
        cwd,
        resolved_model,
        session_id,
        context,
        max_tool_concurrency,
        max_agent_concurrency,
        mode_selection,
        _mcp_manager: tooling.mcp_manager,
    }
}

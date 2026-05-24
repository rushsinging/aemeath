mod concurrency;
mod model_runtime;
mod permissions;

use super::{chat_mode_selection, prompt, runtime, ChatModeSelection};
use crate::agent_runner;
use crate::cli::Args;
use crate::logging_setup::{init_logging, set_session_id};
use crate::mcp_loader::spawn_mcp_connect;
use crate::model_selection::select_model_for_run;
use crate::prompt::{build_system_prompt_parts, PromptContext};
use aemeath_core::config::models::ResolvedModel;
use aemeath_core::logging::{self, JsonLogger};
use aemeath_core::mcp_manager::McpConnectionManager;
use aemeath_core::provider::ApiDriverKind;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::{LlmClient, OpenAIProviderConfig};
use aemeath_llm::providers::openai_compatible::ReasoningConfig;
use concurrency::resolve_concurrency_limits;
use model_runtime::{resolve_model_runtime_settings, ReasoningConfigInput};
use permissions::apply_config_permission_mode;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    aemeath_core::guidance::init_guidance_dir();

    let cwd = args
        .cwd
        .clone()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let config_file = aemeath_core::config::ConfigManager::new(Some(&cwd))
        .load()
        .await
        .ok();

    // 初始化日志系统（在 config 加载之后，使用配置中的日志级别）
    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&aemeath_core::config::LoggingConfig::default()),
    );

    apply_config_permission_mode(&mut args, config_file.as_ref());

    let requested_model = args.model.as_deref();
    let resolved_model = select_model_for_run(requested_model, config_file.as_ref())
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let api_type = resolved_model.api;

    // 获取 API key: CLI args > env vars > resolved config
    let api_key = args.api_key.take().unwrap_or_else(|| {
        let driver_env = match api_type {
            ApiDriverKind::Anthropic => Some("ANTHROPIC_API_KEY"),
            ApiDriverKind::OpenAI => Some("OPENAI_API_KEY"),
            ApiDriverKind::Volcengine => Some("VOLCENGINE_CODING_PLAN_API_KEY"),
            ApiDriverKind::Zhipu | ApiDriverKind::LiteLLM => None,
        };
        std::env::var("AEMEATH_API_KEY")
            .ok()
            .or_else(|| driver_env.and_then(|name| std::env::var(name).ok()))
            .or_else(|| std::env::var("LLM_API_KEY").ok())
            .or_else(|| {
                if !resolved_model.source_config.api_key.is_empty() {
                    Some(resolved_model.source_config.api_key.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                eprintln!("Error: API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json");
                std::process::exit(1);
            })
    });

    let base_url = args.base_url.clone().or_else(|| {
        if resolved_model.source_config.base_url.is_empty() {
            None
        } else {
            Some(resolved_model.source_config.base_url.clone())
        }
    });
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

    let openai_config = if matches!(api_type, ApiDriverKind::Anthropic) {
        None
    } else {
        Some(OpenAIProviderConfig::from_api_driver(
            api_type,
            &resolved_model.source_key,
        ))
    };
    let reasoning_config = runtime_settings
        .reasoning_effort
        .as_ref()
        .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })))
        .or_else(|| {
            if runtime_settings.thinking_max_tokens > 0 {
                Some(ReasoningConfig::ThinkingBudget(
                    runtime_settings.thinking_max_tokens,
                ))
            } else {
                resolved_model.model.reasoning.map(ReasoningConfig::Bool)
            }
        });

    let client = LlmClient::from_config(
        api_type,
        api_key,
        base_url,
        model.clone(),
        runtime_settings.max_tokens,
        runtime_settings.thinking_max_tokens,
        runtime_settings.reasoning,
        reasoning_config,
        openai_config,
    );
    if let Some(effort) = runtime_settings.reasoning_effort {
        client.set_reasoning_effort(Some(effort));
    }

    let client = std::sync::Arc::new(client);

    let task_store = std::sync::Arc::new(aemeath_core::task::TaskStore::new());

    // 加载 skills
    let skill_dirs = config_file
        .as_ref()
        .map(|c| c.skills.dirs.clone())
        .unwrap_or_default();
    let skills_map = aemeath_core::skill::load_all_skills(&cwd, &skill_dirs);
    if !skills_map.is_empty() {
        log::info!("[Skills] loaded {} skills", skills_map.len());
    }
    let skills = std::sync::Arc::new(tokio::sync::Mutex::new(skills_map.clone()));
    let registry = ToolRegistry::new();
    aemeath_tools::register_all_tools(&registry, task_store.clone(), skills.clone());

    let registry = Arc::new(registry);
    let _mcp_manager = spawn_mcp_connect(registry.clone(), &cwd).await;

    // Create hook runner before agent_runner so it can be shared
    let cwd_str = cwd.display().to_string();
    let hook_runner = if let Some(ref cfg) = config_file {
        aemeath_core::hook::HookRunner::from_config(cfg, cwd_str.clone())
    } else {
        aemeath_core::hook::HookRunner::empty(cwd_str.clone())
    };

    // 确定 session ID（尽早生成，以便分化日志、agent_runner 等使用）
    let session_id = args
        .resume
        .clone()
        .unwrap_or_else(aemeath_core::session::new_session_id);
    set_session_id(session_id.clone());
    log::info!("session started");

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
    let prompt_context = PromptContext::new(
        &cwd,
        Some(client.provider_name()),
        Some(client.model_name()),
    );
    let prompt_parts =
        build_system_prompt_parts(&prompt_context, &hook_runner, &prompt_memory_config).await;

    let static_prompt = prompt::build_static_prompt(
        &cwd,
        &model,
        runtime_settings.reasoning,
        config_file.as_ref(),
        &hook_runner,
        prompt_parts.clone(),
        &skills,
    )
    .await;
    // 构建 SystemBlock 数组用于 prompt caching
    use aemeath_llm::types::SystemBlock;
    let system_blocks: Vec<SystemBlock> = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];

    // CLAUDE.md 上下文将作为 user message 前置注入
    let user_context = prompt_parts.claude_md;

    let system_prompt_text = runtime::system_prompt_text(&system_blocks);
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
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        agent_runner,
        task_store,
        skills_map,
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
        _mcp_manager,
    }
}

pub(super) fn build_json_logger(
    session_id: &str,
    config_file: Option<&aemeath_core::config::Config>,
) -> Option<Arc<Mutex<JsonLogger>>> {
    if !config_file
        .map(|c| c.logging.role_logs_enabled)
        .unwrap_or(true)
    {
        return None;
    }

    let logs_dir = config_file
        .and_then(|c| c.logging.logs_dir.as_ref())
        .map(|d| {
            if d.starts_with('~') {
                let home = dirs::home_dir().unwrap_or_default();
                PathBuf::from(d.replacen('~', &home.to_string_lossy(), 1))
            } else {
                PathBuf::from(d)
            }
        })
        .unwrap_or_else(|| logging::log_dir().join("logs"));
    let logging_cfg = config_file.map(|c| &c.logging).cloned().unwrap_or_default();
    match JsonLogger::new(session_id, &logs_dir, &logging_cfg) {
        Ok(jl) => Some(Arc::new(Mutex::new(jl))),
        Err(e) => {
            log::warn!("无法创建分化日志: {}", e);
            None
        }
    }
}

pub(super) fn build_agent_runner(
    config_file: Option<&aemeath_core::config::Config>,
    client: Arc<LlmClient>,
    hook_runner: aemeath_core::hook::HookRunner,
    reasoning: bool,
    json_logger: Option<Arc<Mutex<JsonLogger>>>,
) -> Arc<agent_runner::CliAgentRunner> {
    let models_config_arc = Arc::new(config_file.map(|c| c.models.clone()).unwrap_or_default());
    let has_multi_providers = models_config_arc.providers.len() > 1
        || !config_file
            .map(|c| c.agents.roles.is_empty())
            .unwrap_or(true);

    let pool = if has_multi_providers {
        Some(Arc::new(aemeath_llm::LlmClientPool::new(
            client.clone(),
            models_config_arc.clone(),
        )))
    } else {
        None
    };
    let agents_config = Arc::new(config_file.map(|c| c.agents.clone()).unwrap_or_default());

    Arc::new(agent_runner::CliAgentRunner {
        client: client.clone(),
        pool,
        agents_config,
        hook_runner,
        reasoning,
        models_config: models_config_arc,
        json_logger,
    })
}

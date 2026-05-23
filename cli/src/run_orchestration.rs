mod prompt;
mod runtime;
mod setup;

use crate::cli::Args;
use crate::logging_setup::{init_logging, set_session_id};
use crate::mcp_loader::spawn_mcp_connect;
use crate::model_selection::select_model_for_run;
use crate::prompt::{build_system_prompt_parts, PromptContext};
use aemeath_core::provider::ApiDriverKind;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::{LlmClient, OpenAIProviderConfig};
use aemeath_llm::providers::openai_compatible::ReasoningConfig;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

/// 主聊天逻辑（原 main 主体）
pub(crate) async fn run_chat(mut args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    aemeath_core::command::commands::init_all();

    // 检查 AEMEATH_PERMISSION_MODE 环境变量
    if !args.allow_all {
        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            if mode == "allow_all" {
                args.allow_all = true;
            }
        }
    }

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

    // 应用 config.json 中的 permissions.mode（CLI --allow-all 和 env var 优先）
    if !args.allow_all {
        if let Some(ref cfg) = config_file {
            if matches!(
                cfg.permissions.mode,
                aemeath_core::config::PermissionModeConfig::AllowAll
            ) {
                args.allow_all = true;
            }
        }
    }

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
    let max_tokens = args.max_tokens.unwrap_or_else(|| {
        if resolved_model.model.max_tokens > 0 {
            resolved_model.model.max_tokens
        } else if config_file
            .as_ref()
            .map(|c| c.model.max_tokens > 0)
            .unwrap_or(false)
        {
            config_file.as_ref().unwrap().model.max_tokens
        } else {
            32000
        }
    });
    let thinking_max_tokens = resolved_model.model.thinking_max_tokens;
    let reasoning = resolved_model.model.reasoning.unwrap_or(!args.no_think);

    // reasoning_effort: CLI args > config.json model entry > env var > None
    let reasoning_effort = args
        .reasoning_effort
        .clone()
        .or_else(|| resolved_model.model.reasoning_effort.clone())
        .or_else(|| std::env::var("AEMEATH_REASONING_EFFORT").ok())
        .filter(|e| !e.is_empty());
    if let Some(ref effort) = reasoning_effort {
        if let Err(e) = aemeath_core::config::models::validate_reasoning_effort(effort) {
            log::error!("{}", e);
            std::process::exit(1);
        }
    }

    log::info!(
        "[main] source={} api={} model={} reasoning={} effort={:?} args.no_think={}",
        resolved_model.source_key,
        api_type.as_str(),
        model,
        reasoning,
        reasoning_effort,
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
    let reasoning_config = reasoning_effort
        .as_ref()
        .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })))
        .or_else(|| {
            if thinking_max_tokens > 0 {
                Some(ReasoningConfig::ThinkingBudget(thinking_max_tokens))
            } else {
                resolved_model.model.reasoning.map(ReasoningConfig::Bool)
            }
        });

    let client = LlmClient::from_config(
        api_type,
        api_key,
        base_url,
        model.clone(),
        max_tokens,
        thinking_max_tokens,
        reasoning,
        reasoning_config,
        openai_config,
    );
    if let Some(effort) = reasoning_effort {
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
        .unwrap_or_else(|| aemeath_core::session::new_session_id());
    set_session_id(session_id.clone());
    log::info!("session started");

    let json_logger = setup::build_json_logger(&session_id, config_file.as_ref());
    let agent_runner = setup::build_agent_runner(
        config_file.as_ref(),
        client.clone(),
        hook_runner.clone(),
        reasoning,
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
        reasoning,
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
    // 解析并发限制: CLI args > config file > defaults
    let max_tool_concurrency = args
        .max_tool_concurrency
        .filter(|&v| v > 0)
        .or_else(|| {
            config_file
                .as_ref()
                .map(|c| c.tools.max_concurrency)
                .filter(|&v| v > 0)
        })
        .unwrap_or(10);
    let max_agent_concurrency = args
        .max_agent_concurrency
        .filter(|&v| v > 0)
        .or_else(|| {
            config_file
                .as_ref()
                .map(|c| c.agents.max_concurrency)
                .filter(|&v| v > 0)
        })
        .unwrap_or(4);
    let agent_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));

    log::info!(
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    if args.no_tui || !args.tui {
        let memory_config = config_file
            .as_ref()
            .map(|c| c.memory.clone())
            .unwrap_or_default();
        runtime::run_no_tui(
            client,
            registry,
            system_blocks.clone(),
            system_prompt_text.clone(),
            user_context.clone(),
            cwd,
            &args,
            agent_runner,
            task_store.clone(),
            max_tool_concurrency,
            agent_semaphore.clone(),
            skills_map.clone(),
            hook_runner.clone(),
            memory_config,
            json_logger.clone(),
        )
        .await;
    } else {
        let model_display = runtime::model_display(
            &resolved_model.source_key,
            &resolved_model.model.name,
            &resolved_model.model.id,
        );
        let memory_config = config_file
            .as_ref()
            .map(|c| c.memory.clone())
            .unwrap_or_default();
        runtime::run_tui(
            session_id,
            client,
            registry,
            system_blocks,
            system_prompt_text,
            user_context,
            cwd,
            model_display,
            args,
            agent_runner,
            task_store,
            skills_map,
            hook_runner,
            memory_config,
            json_logger,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
        )
        .await;
    }
}

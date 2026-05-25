mod prompt_bundle;
mod tooling;

use crate::cli::Args;
use crate::model_selection::select_model_for_run;
use ::runtime::api::bootstrap::init_logging;
use prompt_bundle::build_chat_prompt_bundle;
use std::env;
use std::path::PathBuf;
use tooling::build_chat_tooling;

use crate::application::chat::ChatRuntimeContext;
pub(super) use ::runtime::api::bootstrap::ChatBootstrap;
use ::runtime::api::bootstrap::{
    apply_config_permission_mode, build_agent_runner, build_hook_runner, build_json_logger,
    build_llm_client, resolve_api_key, resolve_base_url, resolve_concurrency_limits,
    resolve_model_runtime_settings, ChatBootstrapArgs, ReasoningConfigInput,
};

pub(super) async fn bootstrap_chat(args: Args) -> ChatBootstrap {
    bootstrap_chat_runtime(args.into()).await
}

async fn bootstrap_chat_runtime(mut args: ChatBootstrapArgs) -> ChatBootstrap {
    ::runtime::api::prompt::guidance::init_guidance_dir();

    let cwd = args
        .cwd
        .clone()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let config_file = ::runtime::api::core::config::ConfigManager::new(Some(&cwd))
        .load()
        .await
        .ok();

    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&::runtime::api::core::config::LoggingConfig::default()),
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

    let task_store = std::sync::Arc::new(::runtime::api::core::task::TaskStore::new());
    let tooling = build_chat_tooling(
        &cwd,
        config_file.as_ref().map(|config| &config.skills),
        task_store.clone(),
    )
    .await;

    let hook_runner = build_hook_runner(config_file.as_ref(), &cwd);
    let session_id = start_session_and_cli_log(args.resume.clone());

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
    let mode_selection = args.mode_selection();
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

fn start_session_and_cli_log(resume_session_id: Option<String>) -> String {
    let session_id = ::runtime::api::bootstrap::start_session(resume_session_id);
    ::runtime::api::bootstrap::set_session_id(session_id.clone());
    session_id
}

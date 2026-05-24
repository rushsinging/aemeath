use crate::application::chat::{
    ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, ChatRuntimePort,
    NoTuiChatDependencies, TuiChatDependencies, TuiChatOutcome,
};
use crate::{repl, tui};
use aemeath_core::task::TaskStore;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct NoTuiChatRuntimeAdapter;

#[async_trait(?Send)]
impl ChatRuntimePort for NoTuiChatRuntimeAdapter {
    async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        repl::run_repl(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.cwd,
            request.verbose,
            request.markdown,
            request.context_size,
            request.resume,
            Some(dependencies.agent_runner),
            request.allow_all,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.agent_semaphore,
            dependencies.skills_map,
            dependencies.hook_runner,
            dependencies.memory_config,
            dependencies.json_logger,
        )
        .await;
        Ok(())
    }

    async fn run_tui_chat(
        &self,
        _request: ChatLaunchRequest,
        _dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        Err("NoTuiChatRuntimeAdapter 不支持 TUI 启动".to_string())
    }
}

struct TuiChatRuntimeAdapter;

#[async_trait(?Send)]
impl ChatRuntimePort for TuiChatRuntimeAdapter {
    async fn run_no_tui_chat(
        &self,
        _request: ChatLaunchRequest,
        _dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Err("TuiChatRuntimeAdapter 不支持 no-TUI 启动".to_string())
    }

    async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        let session_id = request
            .session_id
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 session_id".to_string())?;
        let model_display = request
            .model_display
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 model_display".to_string())?;
        let mut app = tui::App::new(session_id.clone(), request.cwd, model_display);
        app.memory_config = dependencies.memory_config;
        app.set_skills(dependencies.skills_map);
        app.hook_runner = dependencies.hook_runner;
        app.json_logger = dependencies.json_logger;
        app.run(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.context_size,
            request.verbose,
            request.markdown,
            Some(dependencies.agent_runner),
            request.allow_all,
            request.resume,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.max_agent_concurrency,
            dependencies.agent_semaphore,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(TuiChatOutcome { session_id })
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_no_tui(
    client: Arc<LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    cwd: PathBuf,
    args: &crate::cli::Args,
    agent_runner: Arc<dyn aemeath_core::tool::AgentRunner>,
    task_store: Arc<TaskStore>,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    skills_map: std::collections::HashMap<String, aemeath_core::skill::Skill>,
    hook_runner: aemeath_core::hook::HookRunner,
    memory_config: aemeath_core::config::MemoryConfig,
    json_logger: Option<Arc<Mutex<aemeath_core::logging::JsonLogger>>>,
) {
    let request = ChatLaunchRequest {
        mode: ChatLaunchMode::NoTui,
        session_id: None,
        cwd,
        model_display: None,
        verbose: args.verbose,
        markdown: !args.no_markdown,
        context_size: args.context_size,
        resume: args.resume.clone(),
        allow_all: args.allow_all,
        max_tool_concurrency,
        max_agent_concurrency,
    };
    let dependencies = NoTuiChatDependencies {
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        agent_runner,
        task_store,
        agent_semaphore,
        skills_map,
        hook_runner,
        memory_config,
        json_logger,
    };
    let service = ChatApplicationService::new(NoTuiChatRuntimeAdapter);
    if let Err(e) = service.run_no_tui_chat(request, dependencies).await {
        log::error!("no-TUI chat application service error: {e}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_tui(
    session_id: String,
    client: Arc<LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    cwd: PathBuf,
    model_display: String,
    args: crate::cli::Args,
    agent_runner: Arc<dyn aemeath_core::tool::AgentRunner>,
    task_store: Arc<TaskStore>,
    skills_map: std::collections::HashMap<String, aemeath_core::skill::Skill>,
    hook_runner: aemeath_core::hook::HookRunner,
    memory_config: aemeath_core::config::MemoryConfig,
    json_logger: Option<Arc<Mutex<aemeath_core::logging::JsonLogger>>>,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
) {
    let request = ChatLaunchRequest {
        mode: ChatLaunchMode::Tui,
        session_id: Some(session_id.clone()),
        cwd,
        model_display: Some(model_display),
        verbose: args.verbose,
        markdown: !args.no_markdown,
        context_size: args.context_size,
        resume: args.resume,
        allow_all: args.allow_all,
        max_tool_concurrency,
        max_agent_concurrency,
    };
    let dependencies = TuiChatDependencies {
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
        max_agent_concurrency,
        agent_semaphore,
    };
    let service = ChatApplicationService::new(TuiChatRuntimeAdapter);
    match service.run_tui_chat(request, dependencies).await {
        Ok(outcome) => println!("aemeath --resume {}", outcome.session_id),
        Err(e) => {
            log::error!("TUI chat application service error: {e}");
            std::process::exit(1);
        }
    }
}

pub(super) fn model_display(source_key: &str, model_name: &str, model_id: &str) -> String {
    let display_name = if model_name.is_empty() {
        model_id
    } else {
        model_name
    };
    format!("{}/{}", source_key, display_name)
}

pub(super) fn system_prompt_text(system_blocks: &[SystemBlock]) -> String {
    system_blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

use crate::application::chat::{
    ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, NoTuiChatDependencies,
    TuiChatDependencies,
};
use aemeath_core::task::TaskStore;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    if let Err(e) = ChatApplicationService::run_no_tui_chat(request, dependencies).await {
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
    match ChatApplicationService::run_tui_chat(request, dependencies).await {
        Ok(session_id) => println!("aemeath --resume {}", session_id),
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

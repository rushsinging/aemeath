use crate::{repl, tui};
use aemeath_core::task::TaskStore;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_no_tui(
    client: Arc<LlmClient>,
    registry: ToolRegistry,
    system_blocks: Vec<SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    cwd: PathBuf,
    args: &crate::cli::Args,
    agent_runner: Arc<dyn aemeath_core::tool::AgentRunner>,
    task_store: Arc<TaskStore>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    skills_map: std::collections::HashMap<String, aemeath_core::skill::Skill>,
    hook_runner: aemeath_core::hook::HookRunner,
    memory_config: aemeath_core::config::MemoryConfig,
    json_logger: Option<Arc<Mutex<aemeath_core::logging::JsonLogger>>>,
) {
    repl::run_repl(
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        cwd,
        args.verbose,
        !args.no_markdown,
        args.context_size,
        args.resume.clone(),
        Some(agent_runner),
        args.allow_all,
        task_store,
        max_tool_concurrency,
        agent_semaphore,
        skills_map,
        hook_runner,
        memory_config,
        json_logger,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_tui(
    session_id: String,
    client: Arc<LlmClient>,
    registry: ToolRegistry,
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
    let mut app = tui::App::new(session_id.clone(), cwd, model_display);
    app.memory_config = memory_config;
    app.set_skills(skills_map);
    app.hook_runner = hook_runner;
    app.json_logger = json_logger;
    if let Err(e) = app
        .run(
            client,
            registry,
            system_blocks,
            system_prompt_text,
            user_context,
            args.context_size,
            args.verbose,
            !args.no_markdown,
            Some(agent_runner),
            args.allow_all,
            args.resume,
            task_store,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
        )
        .await
    {
        log::error!("TUI error: {e}");
        std::process::exit(1);
    }
    println!("aemeath --resume {}", session_id);
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

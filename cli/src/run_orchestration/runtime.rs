use crate::application::chat::{
    ChatApplicationService, ChatLaunchOptions, ChatRuntimeContext, ChatRuntimePort,
    NoTuiChatLaunch, TuiChatLaunch, TuiChatOutcome,
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
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String> {
        repl::run_repl(
            context.client,
            context.registry,
            context.system_blocks,
            context.system_prompt_text,
            context.user_context,
            launch.options.cwd,
            launch.options.verbose,
            launch.options.markdown,
            launch.options.context_size,
            launch.options.resume,
            Some(context.agent_runner),
            launch.options.allow_all,
            context.task_store,
            launch.options.max_tool_concurrency,
            context.agent_semaphore,
            context.skills_map,
            context.hook_runner,
            context.memory_config,
            context.json_logger,
        )
        .await;
        Ok(())
    }

    async fn run_tui_chat(
        &self,
        _launch: TuiChatLaunch,
        _context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        Err("NoTuiChatRuntimeAdapter 不支持 TUI 启动".to_string())
    }
}

struct TuiChatRuntimeAdapter;

#[async_trait(?Send)]
impl ChatRuntimePort for TuiChatRuntimeAdapter {
    async fn run_no_tui_chat(
        &self,
        _launch: NoTuiChatLaunch,
        _context: ChatRuntimeContext,
    ) -> Result<(), String> {
        Err("TuiChatRuntimeAdapter 不支持 no-TUI 启动".to_string())
    }

    async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        let session_id = launch.session_id;
        let mut app = tui::App::new(session_id.clone(), launch.options.cwd, launch.model_display);
        app.memory_config = context.memory_config;
        app.set_skills(context.skills_map);
        app.hook_runner = context.hook_runner;
        app.json_logger = context.json_logger;
        app.run(
            context.client,
            context.registry,
            context.system_blocks,
            context.system_prompt_text,
            context.user_context,
            launch.options.context_size,
            launch.options.verbose,
            launch.options.markdown,
            Some(context.agent_runner),
            launch.options.allow_all,
            launch.options.resume,
            context.task_store,
            launch.options.max_tool_concurrency,
            launch.options.max_agent_concurrency,
            context.agent_semaphore,
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
    let launch = NoTuiChatLaunch {
        options: ChatLaunchOptions {
            cwd,
            verbose: args.verbose,
            markdown: !args.no_markdown,
            context_size: args.context_size,
            resume: args.resume.clone(),
            allow_all: args.allow_all,
            max_tool_concurrency,
            max_agent_concurrency,
        },
    };
    let context = ChatRuntimeContext {
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
    if let Err(e) = service.run_no_tui_chat(launch, context).await {
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
    let launch = TuiChatLaunch {
        options: ChatLaunchOptions {
            cwd,
            verbose: args.verbose,
            markdown: !args.no_markdown,
            context_size: args.context_size,
            resume: args.resume.clone(),
            allow_all: args.allow_all,
            max_tool_concurrency,
            max_agent_concurrency,
        },
        session_id,
        model_display,
    };
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
    let service = ChatApplicationService::new(TuiChatRuntimeAdapter);
    match service.run_tui_chat(launch, context).await {
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

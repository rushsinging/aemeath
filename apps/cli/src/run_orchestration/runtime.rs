use super::setup::ChatBootstrap;
use crate::application::chat::{
    ChatApplicationService, ChatLaunchOptions, ChatRuntimeContext, ChatRuntimePort,
    NoTuiChatLaunch, TuiChatLaunch, TuiChatOutcome,
};
use crate::{repl, tui};
use ::runtime::api::provider::types::SystemBlock;
use async_trait::async_trait;

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
            launch.max_agent_concurrency,
            context.agent_semaphore,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(TuiChatOutcome { session_id })
    }
}

pub(super) async fn run_no_tui_from_bootstrap(bootstrap: ChatBootstrap) {
    let launch = NoTuiChatLaunch {
        options: ChatLaunchOptions {
            cwd: bootstrap.cwd,
            verbose: bootstrap.args.verbose,
            markdown: !bootstrap.args.no_markdown,
            context_size: bootstrap.args.context_size,
            resume: bootstrap.args.resume.clone(),
            allow_all: bootstrap.args.allow_all,
            max_tool_concurrency: bootstrap.max_tool_concurrency,
        },
    };
    let service = ChatApplicationService::new(NoTuiChatRuntimeAdapter);
    if let Err(e) = service.run_no_tui_chat(launch, bootstrap.context).await {
        log::error!("no-TUI chat application service error: {e}");
        std::process::exit(1);
    }
}

pub(super) async fn run_tui_from_bootstrap(bootstrap: ChatBootstrap) {
    let model_display = model_display(
        &bootstrap.resolved_model.source_key,
        &bootstrap.resolved_model.model.name,
        &bootstrap.resolved_model.model.id,
    );
    let launch = TuiChatLaunch {
        options: ChatLaunchOptions {
            cwd: bootstrap.cwd,
            verbose: bootstrap.args.verbose,
            markdown: !bootstrap.args.no_markdown,
            context_size: bootstrap.args.context_size,
            resume: bootstrap.args.resume.clone(),
            allow_all: bootstrap.args.allow_all,
            max_tool_concurrency: bootstrap.max_tool_concurrency,
        },
        max_agent_concurrency: bootstrap.max_agent_concurrency,
        session_id: bootstrap.session_id,
        model_display,
    };
    let service = ChatApplicationService::new(TuiChatRuntimeAdapter);
    match service.run_tui_chat(launch, bootstrap.context).await {
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

use crate::render::TerminalRenderer;
use aemeath_core::compact;
use aemeath_core::message::Message;
use aemeath_core::session::{self, Session};
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;

mod commands;
mod compact_handler;
mod compaction;
mod context;
mod image_input;
mod input;
mod lifecycle;
mod streaming;
mod tool_execution;
mod tools;
mod turns;

use compact_handler::SilentCompactHandler;
use compaction::compact_messages_inner;
use input::InputAction;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Pending images to be attached to the next message
pub(crate) type PendingImages = std::sync::Arc<std::sync::Mutex<Vec<crate::image::ProcessedImage>>>;

#[allow(clippy::too_many_arguments)]
pub async fn run_repl(
    client: Arc<LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<SystemBlock>,
    system_prompt_text: String,
    mut user_context: String,
    cwd: PathBuf,
    verbose: bool,
    markdown: bool,
    context_size: usize,
    resume_id: Option<String>,
    agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    mut allow_all: bool,
    task_store: Arc<TaskStore>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    skills: std::collections::HashMap<String, Skill>,
    hook_runner: aemeath_core::hook::HookRunner,
    memory_config: aemeath_core::config::MemoryConfig,
    json_logger: Option<Arc<std::sync::Mutex<aemeath_core::logging::JsonLogger>>>,
) {
    lifecycle::run_session_start_hooks(&hook_runner, &mut user_context).await;

    let mut rl = match rustyline::DefaultEditor::new() {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("failed to initialize input: {e}");
            return;
        }
    };

    let mut messages = Vec::new();
    let mut total_input_tokens = 0;
    let mut total_output_tokens = 0;
    let mut total_api_calls = 0;
    let mut compact_state = compact::AutoCompactState::default();

    let mut session_id = session::new_session_id();
    let mut resumed_session =
        resume_session(resume_id.as_deref(), &mut messages, &mut session_id).await;

    TerminalRenderer::print_welcome();

    let interrupted = Arc::new(AtomicBool::new(false));
    let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let pending_images: PendingImages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut turn_count = 0usize;
    let session_reminders = Arc::new(std::sync::Mutex::new(
        aemeath_core::memory::SessionReminders::new(),
    ));

    loop {
        match input::read_and_prepare_input(
            &mut rl,
            &mut messages,
            &system_prompt_text,
            context_size,
            total_input_tokens,
            total_output_tokens,
            total_api_calls,
            &session_id,
            &cwd,
            &pending_images,
            resumed_session.as_ref(),
            &mut allow_all,
            &skills,
        )
        .await
        {
            InputAction::Continue => continue,
            InputAction::Exit => break,
            InputAction::Ready => {}
        }

        // Refresh tool schemas each turn so dynamically registered MCP tools
        // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas();
        let tool_schema_tokens = compact::estimate_tool_schemas_tokens(&tool_schemas);

        compact_before_api(
            &mut messages,
            &system_prompt_text,
            context_size,
            tool_schema_tokens,
            &client,
            &hook_runner,
            turn_count,
            &mut compact_state,
            &read_files,
        )
        .await;

        let turn_start = std::time::Instant::now();
        let turn_result = turns::run_agent_turns(
            &mut messages,
            &user_context,
            &system_blocks,
            &system_prompt_text,
            &tool_schemas,
            tool_schema_tokens,
            context_size,
            &client,
            &registry,
            &cwd,
            &interrupted,
            &read_files,
            &agent_runner,
            allow_all,
            max_tool_concurrency,
            &agent_semaphore,
            &session_id,
            &session_reminders,
            &task_store,
            &hook_runner,
            &memory_config,
            &json_logger,
            &mut compact_state,
            turn_count,
            verbose,
            markdown,
        )
        .await;

        total_input_tokens += turn_result.input_tokens;
        total_output_tokens += turn_result.output_tokens;
        total_api_calls += turn_result.api_calls;
        turn_count += turn_result.turns;

        TerminalRenderer::print_done(turn_start.elapsed());
        if let Ok(reminders) = session_reminders.lock() {
            if let Some(line) = reminders.recap_line() {
                eprintln!("{line}");
            }
        }
        TerminalRenderer::print_newline();
    }

    lifecycle::save_session_on_exit(&messages, resumed_session.take(), &session_id, &cwd).await;
    lifecycle::run_stop_hooks(&hook_runner, turn_count).await;
    lifecycle::run_session_end_hooks(&hook_runner).await;
    TerminalRenderer::print_goodbye();
}

async fn resume_session(
    resume_id: Option<&str>,
    messages: &mut Vec<Message>,
    session_id: &mut String,
) -> Option<Session> {
    let id = resume_id?;
    match session::load_session(id).await {
        Ok(session) => {
            let msg_count = session.messages.len();
            *messages = session.messages.clone();
            aemeath_core::message::sanitize_messages(messages);
            let trimmed = msg_count - messages.len();
            let auto_repaired = repair_message_integrity(messages);
            *session_id = session.id.clone();
            TerminalRenderer::print_resumed_session(session_id, msg_count);
            print_resume_repairs(trimmed, auto_repaired);
            Some(session)
        }
        Err(e) => {
            eprintln!("warning: {e}, starting new session");
            None
        }
    }
}

fn repair_message_integrity(messages: &mut Vec<Message>) -> usize {
    let integrity = aemeath_core::message::check_message_integrity(messages);
    if integrity.has_issues() {
        aemeath_core::message::deep_clean_messages(messages)
    } else {
        0
    }
}

fn print_resume_repairs(trimmed: usize, auto_repaired: usize) {
    if trimmed > 0 {
        eprintln!("  [trimmed {} incomplete tool-call message(s)]", trimmed);
    }
    if auto_repaired > 0 {
        eprintln!(
            "  [repaired {} message(s): removed orphaned tool results and fixed role ordering]",
            auto_repaired
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn compact_before_api(
    messages: &mut Vec<Message>,
    system_prompt_text: &str,
    context_size: usize,
    tool_schema_tokens: usize,
    client: &LlmClient,
    hook_runner: &aemeath_core::hook::HookRunner,
    turn_count: usize,
    compact_state: &mut compact::AutoCompactState,
    read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) {
    if compact_state.should_attempt()
        && compact::needs_compaction_full(
            messages,
            system_prompt_text,
            context_size,
            tool_schema_tokens,
        )
        && messages.len() > 4
    {
        compact_messages_inner(
            messages,
            system_prompt_text,
            context_size,
            client,
            hook_runner,
            turn_count,
            compact_state,
            read_files,
        )
        .await;
    }
}

use crate::render::TerminalRenderer;
use ::runtime::api::core::agent::Agent;
use ::runtime::api::core::message::Message;
use ::runtime::api::core::tool::{ToolContext, ToolRegistry};
use ::runtime::api::provider::client::LlmClient;
use ::runtime::api::provider::types::{StopReason, SystemBlock};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use super::compaction::compact_messages_inner;
use super::streaming::{log_response, stream_next_response};
use super::tools::format_tool_summary;

const MAX_TURNS: usize = 100;

pub(super) struct TurnRunResult {
    pub turns: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub api_calls: u64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_agent_turns(
    messages: &mut Vec<Message>,
    user_context: &str,
    system_blocks: &[SystemBlock],
    system_prompt_text: &str,
    tool_schemas: &[serde_json::Value],
    tool_schema_tokens: usize,
    context_size: usize,
    client: &Arc<LlmClient>,
    registry: &ToolRegistry,
    cwd: &Path,
    interrupted: &Arc<AtomicBool>,
    read_files: &Arc<Mutex<HashSet<String>>>,
    agent_runner: &Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
    allow_all: bool,
    max_tool_concurrency: usize,
    agent_semaphore: &Arc<tokio::sync::Semaphore>,
    session_id: &str,
    session_reminders: &Arc<Mutex<::runtime::api::core::memory::SessionReminders>>,
    task_store: &Arc<::runtime::api::core::task::TaskStore>,
    hook_runner: &::runtime::api::core::hook::HookRunner,
    memory_config: &::runtime::api::core::config::MemoryConfig,
    json_logger: &Option<Arc<Mutex<::runtime::api::storage::logging::JsonLogger>>>,
    compact_state: &mut ::runtime::api::core::compact::AutoCompactState,
    turn_count: usize,
    verbose: bool,
    markdown: bool,
) -> TurnRunResult {
    let cancel = CancellationToken::new();
    let ctrlc_handle = spawn_ctrlc_handler(interrupted, &cancel);
    let agent = build_agent(
        registry,
        cwd,
        &cancel,
        read_files,
        agent_runner,
        allow_all,
        max_tool_concurrency,
        agent_semaphore,
        session_id,
        session_reminders,
        memory_config,
    );

    let mut result = TurnRunResult {
        turns: 0,
        input_tokens: 0,
        output_tokens: 0,
        api_calls: 0,
    };
    let mut last_api_input_tokens;
    while result.turns < MAX_TURNS {
        result.turns += 1;
        if interrupted.load(Ordering::Acquire) {
            interrupted.store(false, Ordering::Release);
            TerminalRenderer::print_interrupted();
            break;
        }

        let response = stream_next_response(
            client,
            system_blocks,
            messages,
            user_context,
            tool_schemas,
            &cancel,
            verbose,
            markdown,
            json_logger,
            turn_count + result.turns,
        )
        .await;

        let (resp, elapsed) = match response {
            Ok(value) => value,
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("interrupted by user") {
                    TerminalRenderer::print_cancelled();
                } else {
                    eprintln!("error: {e}");
                }
                break;
            }
        };

        if interrupted.load(Ordering::Acquire) {
            interrupted.store(false, Ordering::Release);
            TerminalRenderer::print_interrupted();
            break;
        }

        println!();
        last_api_input_tokens = resp.usage.input_tokens as u64;
        result.input_tokens += last_api_input_tokens;
        result.output_tokens += resp.usage.output_tokens as u64;
        result.api_calls += 1;
        TerminalRenderer::print_usage(resp.usage.input_tokens, resp.usage.output_tokens, elapsed);

        messages.push(resp.assistant_message.clone());
        let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
        log_response(
            json_logger,
            client,
            turn_count + result.turns,
            &resp,
            elapsed,
            &tool_calls,
        );

        if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
            run_reflection(
                memory_config,
                turn_count + result.turns,
                messages,
                cwd,
                client,
                system_prompt_text,
            )
            .await;
            break;
        }

        let call_summaries = build_call_summaries(&tool_calls, task_store).await;
        let results = super::tool_execution::execute_and_render_tools(
            &agent,
            registry,
            &tool_calls,
            &call_summaries,
            task_store,
            &cancel,
            session_id,
            allow_all,
            json_logger,
            client,
            turn_count + result.turns,
        )
        .await;

        append_tool_results(messages, results);
        maybe_compact_after_tools(
            messages,
            last_api_input_tokens,
            context_size,
            system_prompt_text,
            tool_schema_tokens,
            compact_state,
            client,
            hook_runner,
            turn_count,
            read_files,
        )
        .await;
    }

    if result.turns >= MAX_TURNS {
        eprintln!("max turns ({MAX_TURNS}) reached");
    }
    ctrlc_handle.abort();
    interrupted.store(false, Ordering::Release);
    result
}

fn spawn_ctrlc_handler(
    interrupted: &Arc<AtomicBool>,
    cancel: &CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let interrupted_clone = interrupted.clone();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        interrupted_clone.store(true, Ordering::Release);
        cancel_clone.cancel();
    })
}

#[allow(clippy::too_many_arguments)]
fn build_agent<'a>(
    registry: &'a ToolRegistry,
    cwd: &Path,
    cancel: &CancellationToken,
    read_files: &Arc<Mutex<HashSet<String>>>,
    agent_runner: &Option<Arc<dyn ::runtime::api::core::tool::AgentRunner>>,
    allow_all: bool,
    max_tool_concurrency: usize,
    agent_semaphore: &Arc<tokio::sync::Semaphore>,
    session_id: &str,
    session_reminders: &Arc<Mutex<::runtime::api::core::memory::SessionReminders>>,
    memory_config: &::runtime::api::core::config::MemoryConfig,
) -> Agent<'a> {
    let (cwd, working_root, path_base) = ToolContext::new_working_paths(cwd.to_path_buf());
    let ctx = ToolContext {
        cwd,
        working_root,
        path_base,
        cancel: cancel.clone(),
        read_files: read_files.clone(),
        agent_runner: agent_runner.clone(),
        session_reminders: Some(session_reminders.clone()),
        memory_config: memory_config.clone(),
        plan_mode: None,
        allow_all,
        max_tool_concurrency,
        max_agent_concurrency: 0,
        agent_semaphore: agent_semaphore.clone(),
        progress_tx: None,
        parent_session_id: Some(session_id.to_string()),
        context_stack: Arc::new(Mutex::new(Vec::new())),
    };
    Agent { registry, ctx }
}

async fn run_reflection(
    memory_config: &::runtime::api::core::config::MemoryConfig,
    turn_number: usize,
    messages: &[Message],
    cwd: &Path,
    client: &LlmClient,
    system_prompt_text: &str,
) {
    if let Some(text) = ::runtime::api::chat::reflection::run_reflection(
        memory_config,
        turn_number,
        messages,
        &cwd.to_path_buf(),
        client,
        system_prompt_text,
    )
    .await
    {
        eprintln!("{text}");
    }
}

async fn build_call_summaries(
    tool_calls: &[::runtime::api::core::agent::ToolCall],
    task_store: &Arc<::runtime::api::core::task::TaskStore>,
) -> HashMap<String, (String, String)> {
    let pending_tasks = super::tool_execution::pending_task_lines(task_store).await;
    tool_calls
        .iter()
        .map(|call| {
            let summary = if call.name == "TodoRun" && !pending_tasks.is_empty() {
                format!(
                    "{} todo(s)\n{}",
                    pending_tasks.len(),
                    pending_tasks.join("\n")
                )
            } else {
                format_tool_summary(&call.name, &call.input)
            };
            (call.id.clone(), (call.name.clone(), summary))
        })
        .collect()
}

fn append_tool_results(
    messages: &mut Vec<Message>,
    results: Vec<::runtime::api::core::agent::ToolResultTuple>,
) {
    let has_images = results.iter().any(|(_, _, _, imgs)| !imgs.is_empty());
    if has_images {
        messages.push(Message::tool_results_rich(results));
    } else {
        let simple = results
            .into_iter()
            .map(|(id, output, is_error, _)| (id, output, is_error))
            .collect();
        messages.push(Message::tool_results(simple));
    }
}

#[allow(clippy::too_many_arguments)]
async fn maybe_compact_after_tools(
    messages: &mut Vec<Message>,
    last_api_input_tokens: u64,
    context_size: usize,
    system_prompt_text: &str,
    tool_schema_tokens: usize,
    compact_state: &mut ::runtime::api::core::compact::AutoCompactState,
    client: &LlmClient,
    hook_runner: &::runtime::api::core::hook::HookRunner,
    turn_count: usize,
    read_files: &Arc<Mutex<HashSet<String>>>,
) {
    let urgency = if last_api_input_tokens > 0 {
        let new_tokens = messages
            .last()
            .map(|m| {
                ::runtime::api::core::compact::estimate_messages_tokens(std::slice::from_ref(m))
            })
            .unwrap_or(0) as u64;
        ::runtime::api::core::compact::compaction_urgency(
            last_api_input_tokens + new_tokens,
            context_size,
        )
    } else if ::runtime::api::core::compact::needs_compaction_full(
        messages,
        system_prompt_text,
        context_size,
        tool_schema_tokens,
    ) {
        2
    } else {
        0
    };

    if urgency >= 1 && messages.len() > 4 {
        let old_len = messages.len();
        ::runtime::api::core::compact::microcompact(messages, 6);
        if urgency >= 2 && compact_state.should_attempt() {
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
        } else {
            TerminalRenderer::print_compaction(old_len, messages.len());
        }
    }
}

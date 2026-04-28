use crate::image::{is_image_file, process_image_file};
use crate::render::{TerminalRenderer, TerminalStreamHandler, ThinkingIndicator};
use aemeath_core::agent::Agent;
use aemeath_core::compact;
use aemeath_core::message::Message;
use aemeath_core::session::{self, Session};
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{ToolContext, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::{StopReason, SystemBlock};

mod commands;
mod compact_handler;
mod context;
mod image_input;
mod tools;

use commands::{SlashResult, handle_slash_command};
use compact_handler::SilentCompactHandler;
use context::build_user_context_message;
use image_input::extract_image_paths;
use tools::{ask_permission, format_tool_summary};

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const MAX_TURNS: usize = 100;

/// Pending images to be attached to the next message
pub(crate) type PendingImages = std::sync::Arc<std::sync::Mutex<Vec<crate::image::ProcessedImage>>>;

#[allow(unused_assignments)]
pub async fn run_repl(
    client: Arc<LlmClient>,
    registry: ToolRegistry,
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
    _task_store: Arc<TaskStore>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    skills: std::collections::HashMap<String, Skill>,
    hook_runner: aemeath_core::hook::HookRunner,
) {
    // Run SessionStart hooks: inject additional_context into user_context
    {
        use aemeath_core::config::hooks::HookEvent;
        use aemeath_core::hook::{HookData, SessionHookData};
        let hook_results = hook_runner.run_hooks_with_json(
            HookEvent::SessionStart,
            None,
            HookData::Session(SessionHookData {}),
        ).await;
        for (_, result, json_output) in &hook_results {
            if let Some(json) = json_output {
                if let Some(ref ctx) = json.additional_context {
                    user_context = if user_context.is_empty() {
                        ctx.clone()
                    } else {
                        format!("{}\n\n{}", ctx, user_context)
                    };
                }
            }
            if result.blocked {
                eprintln!("[SessionStart hook blocked session start]");
            }
        }
    }

    let mut rl = match DefaultEditor::new() {
        Ok(rl) => rl,
        Err(e) => {
            eprintln!("failed to initialize input: {e}");
            return;
        }
    };

    let mut messages: Vec<Message> = Vec::new();
    let tool_schemas = registry.schemas();
    let tool_schema_tokens = compact::estimate_tool_schemas_tokens(&tool_schemas);
    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;
    let mut total_api_calls: u64 = 0;
    let mut compact_state = compact::AutoCompactState::default();

    let mut session_id = session::new_session_id();
    let mut resumed_session: Option<Session> = None;

    if let Some(ref id) = resume_id {
        match session::load_session(id).await {
            Ok(s) => {
                let msg_count = s.messages.len();
                messages = s.messages.clone();
                aemeath_core::message::sanitize_messages(&mut messages);
                let trimmed = msg_count - messages.len();
                // Check for deeper integrity issues
                let integrity = aemeath_core::message::check_message_integrity(&messages);
                let auto_repaired = if integrity.has_issues() {
                    aemeath_core::message::deep_clean_messages(&mut messages)
                } else {
                    0
                };
                session_id = s.id.clone();
                resumed_session = Some(s);
                TerminalRenderer::print_resumed_session(&session_id, msg_count);
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
            Err(e) => {
                eprintln!("warning: {e}, starting new session");
            }
        }
    }

    TerminalRenderer::print_welcome();

    let interrupted = Arc::new(AtomicBool::new(false));
    let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    let pending_images: PendingImages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut skip_message_build = false;

    loop {
        skip_message_build = false;

        {
            let images = pending_images.lock().unwrap();
            if !images.is_empty() {
                TerminalRenderer::print_pending_images(images.len());
            }
        }

        TerminalRenderer::print_user_prompt();
        let input = match rl.readline("") {
            Ok(line) => line.trim().to_string(),
            Err(ReadlineError::Interrupted) => {
                println!("(use /exit to quit)");
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("input error: {e}");
                break;
            }
        };

        if input.is_empty() {
            continue;
        }

        // Handle slash commands
        if input.starts_with('/') {
            match handle_slash_command(
                &input,
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
            ).await {
                SlashResult::Continue => continue,
                SlashResult::Exit => break,
                SlashResult::NotFound => {
                    eprintln!("unknown command: {input}. Type /help for available commands.");
                    continue;
                }
                SlashResult::Review(prompt) => {
                    messages.push(Message::user(&prompt));
                    skip_message_build = true;
                }
            }
        }

        // Auto-detect image file paths
        if is_image_file(&input) {
            let full_path = if std::path::Path::new(&input).is_absolute() {
                input.clone()
            } else {
                cwd.join(&input).to_string_lossy().to_string()
            };
            match process_image_file(&full_path).await {
                Ok(img) => {
                    let size = img.original_size;
                    pending_images.lock().unwrap().push(img);
                    println!("[image added ({} bytes)]", size);
                    println!("  Type your message and press Enter to send with the image.");
                    continue;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    continue;
                }
            }
        }

        let (clean_input, inline_images) = extract_image_paths(&input, &cwd).await;

        let _ = rl.add_history_entry(&input);

        {
            let mut pending = pending_images.lock().unwrap();
            pending.extend(inline_images);
        }

        if !skip_message_build {
            let images = pending_images.lock().unwrap().drain(..).collect::<Vec<_>>();
            let msg_text = if clean_input.is_empty() { &input } else { &clean_input };
            if images.is_empty() {
                messages.push(Message::user(msg_text));
            } else {
                let image_data: Vec<(String, String)> = images
                    .iter()
                    .map(|img| (img.base64.clone(), img.media_type.clone()))
                    .collect();
                messages.push(Message::user_with_images(msg_text, image_data));
                for (i, img) in images.iter().enumerate() {
                    println!("[sent image {}: {} bytes]", i + 1, img.final_size);
                }
            }
        }

        // Auto-compact before sending to API
        if compact_state.should_attempt()
            && compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
            && messages.len() > 4
        {
            compact_messages_inner(
                &mut messages, &system_prompt_text, context_size,
                &client, &mut compact_state, &read_files,
            ).await;
        }

        let cancel = CancellationToken::new();
        let interrupted_clone = interrupted.clone();
        let cancel_clone = cancel.clone();

        let ctrlc_handle = tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            interrupted_clone.store(true, Ordering::Release);
            cancel_clone.cancel();
        });

        let ctx = ToolContext {
            cwd: cwd.clone(),
            cancel: cancel.clone(),
            read_files: read_files.clone(),
            agent_runner: agent_runner.clone(),
            plan_mode: None,
            allow_all,
            max_tool_concurrency,
            max_agent_concurrency: 0,
            agent_semaphore: agent_semaphore.clone(),
            progress_tx: None,
        };
        let agent = Agent {
            registry: &registry,
            ctx,
        };

        let mut turns = 0;
        let mut last_api_input_tokens: u64 = 0;
        let turn_start = std::time::Instant::now();
        loop {
            if turns >= MAX_TURNS {
                eprintln!("max turns ({MAX_TURNS}) reached");
                break;
            }
            turns += 1;

            if interrupted.load(Ordering::Acquire) {
                interrupted.store(false, Ordering::Release);
                TerminalRenderer::print_interrupted();
                break;
            }

            let messages_for_api: Vec<Message> = {
                let mut api_msgs = Vec::new();
                if let Some(ctx_msg) = build_user_context_message(&user_context) {
                    api_msgs.push(ctx_msg);
                }
                api_msgs.extend(messages.iter().cloned());
                api_msgs
            };

            let indicator = ThinkingIndicator::start("thinking...");
            let mut handler = TerminalStreamHandler::new(verbose, markdown);
            let response = client
                .stream_message(&system_blocks, &messages_for_api, &tool_schemas, &mut handler, &cancel)
                .await;
            let elapsed = indicator.elapsed();
            indicator.stop();

            if interrupted.load(Ordering::Acquire) {
                interrupted.store(false, Ordering::Release);
                TerminalRenderer::print_interrupted();
                break;
            }

            match response {
                Ok(resp) => {
                    println!();
                    last_api_input_tokens = resp.usage.input_tokens as u64;
                    total_input_tokens += last_api_input_tokens;
                    total_output_tokens += resp.usage.output_tokens as u64;
                    total_api_calls += 1;
                    TerminalRenderer::print_usage(resp.usage.input_tokens, resp.usage.output_tokens, elapsed);

                    messages.push(resp.assistant_message.clone());

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        break;
                    }

                    let mut approved_calls: Vec<&aemeath_core::agent::ToolCall> = Vec::new();
                    let mut denied_results: Vec<aemeath_core::agent::ToolResultTuple> = Vec::new();

                    let pending_tasks: Vec<String> = _task_store.list().await
                        .iter()
                        .filter(|t| t.status == aemeath_core::task::TaskStatus::Pending)
                        .map(|t| {
                            let dep = if t.blocked_by.is_empty() {
                                String::new()
                            } else {
                                format!(" (blocked by #{})", t.blocked_by.join(", #"))
                            };
                            format!("  ○ #{} {}{}", t.id, t.subject, dep)
                        })
                        .collect();

                    let call_summaries: std::collections::HashMap<String, (String, String)> = tool_calls
                        .iter()
                        .map(|call| {
                            let summary = if call.name == "TodoRun" {
                                if pending_tasks.is_empty() {
                                    format_tool_summary(&call.name, &call.input)
                                } else {
                                    format!("{} todo(s)\n{}", pending_tasks.len(), pending_tasks.join("\n"))
                                }
                            } else {
                                format_tool_summary(&call.name, &call.input)
                            };
                            (call.id.clone(), (call.name.clone(), summary))
                        })
                        .collect();

                    for call in &tool_calls {
                        let is_safe = if call.name == "Bash" {
                            call.input.get("command")
                                .and_then(|v| v.as_str())
                                .map(aemeath_tools::bash::is_readonly_command)
                                .unwrap_or(false)
                        } else {
                            registry.get(&call.name)
                                .map(|t| t.is_read_only())
                                .unwrap_or(false)
                        };

                        if is_safe || allow_all {
                            approved_calls.push(call);
                        } else {
                            if ask_permission(&call.name) {
                                approved_calls.push(call);
                            } else {
                                denied_results.push((
                                    call.id.clone(),
                                    format!("Tool {} was denied by user", call.name),
                                    true,
                                    Vec::new(),
                                ));
                            }
                        }
                    }

                    // TodoRun progress poller
                    let has_todo_run = approved_calls.iter().any(|c| c.name == "TodoRun");
                    let progress_handle = if has_todo_run {
                        let store = _task_store.clone();
                        let cancel_token = cancel.clone();
                        Some(tokio::spawn(async move {
                            use aemeath_core::task::TaskStatus;
                            let mut last_statuses: std::collections::HashMap<String, TaskStatus> =
                                std::collections::HashMap::new();
                            for t in store.list().await {
                                last_statuses.insert(t.id.clone(), t.status.clone());
                            }
                            loop {
                                tokio::select! {
                                    _ = cancel_token.cancelled() => break,
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                                        let current = store.list().await;
                                        for t in &current {
                                            let prev = last_statuses.get(&t.id);
                                            let changed = match prev {
                                                Some(ps) => *ps != t.status,
                                                None => true,
                                            };
                                            if changed {
                                                match t.status {
                                                    TaskStatus::InProgress => {
                                                        let action = t.active_form.as_deref().unwrap_or("Processing");
                                                        eprintln!("  ◐ {} — {}", t.subject, action);
                                                    }
                                                    TaskStatus::Completed => {
                                                        eprintln!("  ✓ {}", t.subject);
                                                    }
                                                    _ => {}
                                                }
                                                last_statuses.insert(t.id.clone(), t.status.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }))
                    } else {
                        None
                    };

                    let mut results = agent.execute_tools_filtered(&approved_calls).await;
                    results.extend(denied_results);

                    if let Some(handle) = progress_handle {
                        handle.abort();
                    }

                    let persisted = aemeath_core::tool_result_storage::persist_oversized_results(
                        &session_id, &mut results,
                    );
                    if persisted > 0 {
                        println!("[{persisted} tool result(s) persisted to disk]");
                    }

                    for (_id, output, is_error, _images) in results.iter() {
                        if let Some((name, summary)) = call_summaries.get(_id) {
                            TerminalRenderer::print_tool_call(name, summary);
                        }
                        let tool_name = call_summaries.get(_id)
                            .map(|(n, _): &(String, String)| n.as_str())
                            .unwrap_or("");
                        TerminalRenderer::print_tool_result_with_diff(tool_name, output, *is_error);
                    }

                    let has_images = results.iter().any(|(_, _, _, imgs)| !imgs.is_empty());
                    if has_images {
                        messages.push(Message::tool_results_rich(results));
                    } else {
                        let simple: Vec<(String, String, bool)> = results
                            .into_iter()
                            .map(|(id, output, is_error, _)| (id, output, is_error))
                            .collect();
                        messages.push(Message::tool_results(simple));
                    }

                    // Inner-loop auto-compact
                    let urgency = if last_api_input_tokens > 0 {
                        let new_tokens = messages.last()
                            .map(|m| compact::estimate_messages_tokens(std::slice::from_ref(m)))
                            .unwrap_or(0) as u64;
                        compact::compaction_urgency(last_api_input_tokens + new_tokens, context_size)
                    } else if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens) {
                        2
                    } else {
                        0
                    };

                    if urgency >= 1 && messages.len() > 4 {
                        let old_len = messages.len();
                        compact::microcompact(&mut messages, 6);

                        if urgency >= 2 && compact_state.should_attempt() {
                            compact_messages_inner(
                                &mut messages, &system_prompt_text, context_size,
                                &client, &mut compact_state, &read_files,
                            ).await;
                        } else {
                            TerminalRenderer::print_compaction(old_len, messages.len());
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("{e}");
                    if msg.contains("interrupted by user") {
                        TerminalRenderer::print_cancelled();
                    } else {
                        eprintln!("error: {e}");
                    }
                    break;
                }
            }
        }

        ctrlc_handle.abort();
        interrupted.store(false, Ordering::Release);

        TerminalRenderer::print_done(turn_start.elapsed());
        TerminalRenderer::print_newline();
    }

    // Auto-save session on exit
    if !messages.is_empty() {
        let s = if let Some(mut existing) = resumed_session.take() {
            existing.messages = messages.clone();
            existing.updated_at = session::now_iso();
            existing
        } else {
            let mut new_s = Session::new(session_id.clone(), cwd.to_string_lossy().to_string());
            new_s.messages = messages.clone();
            new_s
        };
        if let Err(e) = session::save_session(&s).await {
            eprintln!("warning: failed to save session: {e}");
        } else {
            TerminalRenderer::print_session_saved(&session_id);
        }
    }

    // Run SessionEnd hooks
    {
        let hook_results = hook_runner.on_session_end().await;
        for (_, result, json_output) in &hook_results {
            if let Some(json) = json_output {
                if let Some(ref msg) = json.system_message {
                    eprintln!("{}", msg);
                }
            }
            if result.error.is_some() {
                log::warn!("SessionEnd hook error: {:?}", result.error);
            }
        }
    }

    TerminalRenderer::print_goodbye();
}

/// Shared compaction logic used in both outer and inner loop.
async fn compact_messages_inner(
    messages: &mut Vec<Message>,
    system_prompt_text: &str,
    context_size: usize,
    client: &LlmClient,
    compact_state: &mut compact::AutoCompactState,
    read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) {
    let old_len = messages.len();
    let keep_recent = (old_len * 40 / 100).max(4).min(old_len - 1);
    let split_point = old_len - keep_recent;
    let early_messages = &messages[..split_point];

    let compact_request = compact::build_compact_request(early_messages);
    let compact_system = vec![SystemBlock::dynamic(
        "You are a conversation summarizer. Respond only with the summary.".to_string(),
    )];
    let mut silent_handler = SilentCompactHandler;
    let compact_cancel = CancellationToken::new();
    match client
        .stream_message(&compact_system, &compact_request, &[], &mut silent_handler, &compact_cancel)
        .await
    {
        Ok(compact_resp) => {
            let summary = compact::parse_compact_response(&compact_resp.assistant_message.text_content());
            let recent = messages[split_point..].to_vec();
            let files = read_files.lock().unwrap().clone();
            let (compacted, _) =
                compact::assemble_compacted_with_files(summary, &recent, split_point, Some(&files));
            *messages = compacted;
            compact_state.record_success();
            TerminalRenderer::print_compaction(old_len, messages.len());
        }
        Err(_) => {
            compact_state.record_failure();
            let (compacted, was_compacted) =
                compact::compact_messages(messages, system_prompt_text, context_size);
            if was_compacted {
                *messages = compacted;
                TerminalRenderer::print_compaction(old_len, messages.len());
            }
        }
    }
}

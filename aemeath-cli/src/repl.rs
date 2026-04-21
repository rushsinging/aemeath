use crate::image::{is_image_file, process_image_file, ProcessedImage};
use crate::render::{TerminalRenderer, TerminalStreamHandler, ThinkingIndicator};
use aemeath_core::agent::Agent;
use aemeath_core::command::{cmd, CommandContext, CommandRegistry, CommandResult};
use aemeath_core::compact;
use aemeath_core::message::Message;
use aemeath_core::session::{self, Session};
use aemeath_core::skill::Skill;
use aemeath_core::state::AppState;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{ToolContext, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::stream::StreamHandler;
use aemeath_llm::types::{StopReason, SystemBlock};

/// Silent handler for LLM-based compaction (no terminal output).
struct SilentCompactHandler;
impl StreamHandler for SilentCompactHandler {
    fn on_text(&mut self, _text: &str) {}
    fn on_tool_use_start(&mut self, _name: &str) {}
    fn on_error(&mut self, _error: &str) {}
}
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

const MAX_TURNS: usize = 100;

/// Pending images to be attached to the next message
type PendingImages = std::sync::Arc<std::sync::Mutex<Vec<ProcessedImage>>>;

/// Build the user context message from CLAUDE.md content, wrapped in <system-reminder> tags.
fn build_user_context_message(claude_md: &str) -> Option<Message> {
    if claude_md.is_empty() {
        return None;
    }
    Some(Message::user(format!(
        "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{claude_md}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
    )))
}

#[allow(unused_assignments)]
pub async fn run_repl(
    client: std::sync::Arc<LlmClient>,
    registry: ToolRegistry,
    system_blocks: Vec<SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    cwd: PathBuf,
    verbose: bool,
    markdown: bool,
    context_size: usize,
    resume_id: Option<String>,
    agent_runner: Option<std::sync::Arc<dyn aemeath_core::tool::AgentRunner>>,
    mut allow_all: bool,
    _task_store: Arc<TaskStore>,
    max_tool_concurrency: usize,
    agent_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    skills: std::collections::HashMap<String, Skill>,
) {
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
    // Keep the original session object when resuming, to preserve metadata
    let mut resumed_session: Option<Session> = None;

    // Resume existing session if requested
    if let Some(ref id) = resume_id {
        match session::load_session(id).await {
            Ok(s) => {
                messages = s.messages.clone();
                session_id = s.id.clone();
                resumed_session = Some(s);
                TerminalRenderer::print_resumed_session(&session_id, messages.len());
            }
            Err(e) => {
                eprintln!("warning: {e}, starting new session");
            }
        }
    }

    TerminalRenderer::print_welcome();

    // Set up Ctrl+C handler
    let interrupted = Arc::new(AtomicBool::new(false));

    let read_files = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));

    // Pending images for next message
    let pending_images: PendingImages = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut skip_message_build = false;

    loop {
        // Reset skip flag at the start of each iteration
        skip_message_build = false;

        // Show pending images indicator
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
                    // Treat the review prompt as user input and let the normal agent loop handle it
                    // by falling through to the agent processing below
                    messages.push(Message::user(&prompt));
                    skip_message_build = true;
                    // fall through to agent processing
                }
            }
        }

        // Auto-detect image file paths in the input
        // Case 1: entire input is an image path → add to pending, don't send yet
        if is_image_file(&input) {
            let full_path = if Path::new(&input).is_absolute() {
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

        // Case 2: input contains inline image paths → extract and attach them
        let (clean_input, inline_images) = extract_image_paths(&input, &cwd).await;

        let _ = rl.add_history_entry(&input);

        // Merge inline images into pending images
        {
            let mut pending = pending_images.lock().unwrap();
            pending.extend(inline_images);
        }

        // Build message with any pending images (skip if message was already built by /review)
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

                // Print summary of attached images
                for (i, img) in images.iter().enumerate() {
                    println!(
                        "[sent image {}: {} bytes]",
                        i + 1, img.final_size
                    );
                }
            }
        }

        // Auto-compact before sending to API (guarded by circuit breaker)
        if compact_state.should_attempt()
            && compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
            && messages.len() > 4
        {
            let old_len = messages.len();

            // Step 1: Try microcompact first
            compact::microcompact(&mut messages, 10);

            if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens) {
                // Step 2: LLM-based compaction
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
                    Ok(resp) => {
                        let summary = compact::parse_compact_response(&resp.assistant_message.text_content());
                        let recent = messages[split_point..].to_vec();
                        let files = read_files.lock().unwrap().clone();
                        let (compacted, _) =
                            compact::assemble_compacted_with_files(summary, &recent, split_point, Some(&files));
                        messages = compacted;
                        compact_state.record_success();
                        TerminalRenderer::print_compaction(old_len, messages.len());
                    }
                    Err(_) => {
                        compact_state.record_failure();
                        // Fallback to local compaction
                        let (compacted, was_compacted) =
                            compact::compact_messages(&messages, &system_prompt_text, context_size);
                        if was_compacted {
                            messages = compacted;
                            TerminalRenderer::print_compaction(old_len, messages.len());
                        }
                    }
                }
            } else {
                TerminalRenderer::print_compaction(old_len, messages.len());
            }
        }

        let cancel = CancellationToken::new();
        let interrupted_clone = interrupted.clone();
        let cancel_clone = cancel.clone();

        // Ctrl+C during API call cancels the current request
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
            max_agent_concurrency: 0, // not used directly here
            agent_semaphore: agent_semaphore.clone(),
        };
        let agent = Agent {
            registry: &registry,
            ctx,
        };

        let mut turns = 0;
        let mut last_api_input_tokens: u64 = 0;
        loop {
            if turns >= MAX_TURNS {
                eprintln!("max turns ({MAX_TURNS}) reached");
                break;
            }
            turns += 1;

            // Check if interrupted
            if interrupted.load(Ordering::Acquire) {
                              interrupted.store(false, Ordering::Release);
                TerminalRenderer::print_interrupted();
                break;
            }

            // Rebuild messages_for_api each iteration so the model sees
            // its own prior responses and tool results
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
            indicator.stop();

            // Check if interrupted during API call
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
                    TerminalRenderer::print_usage(resp.usage.input_tokens, resp.usage.output_tokens);

                    messages.push(resp.assistant_message.clone());

                    let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                    if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                        break;
                    }

                    // Show tool calls and ask permission for non-read-only tools
                    let mut approved_calls: Vec<&aemeath_core::agent::ToolCall> = Vec::new();
                    let mut denied_results: Vec<aemeath_core::agent::ToolResultTuple> = Vec::new();

                    // Build call info lookup for interleaved display (call → result)
                    // Pre-fetch pending tasks outside the closure (can't .await in map())
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
                            // For Bash, check the actual command content
                            call.input.get("command")
                                .and_then(|v| v.as_str())
                                .map(aemeath_tools::bash::is_readonly_command)
                                .unwrap_or(false)
                        } else {
                            registry
                                .get(&call.name)
                                .map(|t| t.is_read_only())
                                .unwrap_or(false)
                        };

                        if is_safe || allow_all {
                            approved_calls.push(call);
                        } else {
                            // Ask permission
                            let approved = ask_permission(&call.name);
                            if approved {
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

                    // Spawn TodoRun progress poller if there are TodoRun calls
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

                    // Stop progress poller
                    if let Some(handle) = progress_handle {
                        handle.abort();
                    }

                    // Persist oversized tool results to disk, replace with preview reference
                    let persisted = aemeath_core::tool_result_storage::persist_oversized_results(
                        &session_id, &mut results,
                    );
                    if persisted > 0 {
                        println!("[{persisted} tool result(s) persisted to disk]");
                    }

                    for (_id, output, is_error, _images) in results.iter() {
                        // Show tool call header then its result (interleaved)
                        if let Some((name, summary)) = call_summaries.get(_id) {
                            TerminalRenderer::print_tool_call(name, summary);
                        }
                        let tool_name = call_summaries.get(_id)
                            .map(|(n, _): &(String, String)| n.as_str())
                            .unwrap_or("");
                        TerminalRenderer::print_tool_result_with_diff(tool_name, output, *is_error);
                    }

                    // Check if any results have images
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

                    // [Plan A disabled] Auto-trigger TodoRun is available but currently
                    // disabled in favor of Plan B (Agent tool interception).
                    // To re-enable, uncomment the block in auto_trigger_todorun().
                    // See also: tui/app.rs process_in_background() for the TUI equivalent.

                    // Inner-loop auto-compact: use API-reported token count when available,
                    // fall back to estimation. Urgency levels determine action:
                    // 0 = ok, 1 = microcompact, 2 = full compact, 3 = critical
                    let urgency = if last_api_input_tokens > 0 {
                        // Estimate new tokens added since last API call (tool results)
                        let new_tokens = messages.last()
                            .map(|m| compact::estimate_messages_tokens(std::slice::from_ref(m)))
                            .unwrap_or(0) as u64;
                        compact::compaction_urgency(last_api_input_tokens + new_tokens, context_size)
                    } else if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens) {
                        2 // fallback to estimation-based
                    } else {
                        0
                    };

                    if urgency >= 1 && messages.len() > 4 {
                        let old_len = messages.len();
                        compact::microcompact(&mut messages, 6);

                        if urgency >= 2 && compact_state.should_attempt() {
                            // Full compaction needed
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
                                    messages = compacted;
                                    last_api_input_tokens = 0;
                                    compact_state.record_success();
                                    TerminalRenderer::print_compaction(old_len, messages.len());
                                }
                                Err(_) => {
                                    compact_state.record_failure();
                                    let (compacted, was_compacted) =
                                        compact::compact_messages(&messages, &system_prompt_text, context_size);
                                    if was_compacted {
                                        messages = compacted;
                                        last_api_input_tokens = 0;
                                        TerminalRenderer::print_compaction(old_len, messages.len());
                                    }
                                }
                            }
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

        // Clean up Ctrl+C handler
        ctrlc_handle.abort();
        interrupted.store(false, Ordering::Release);

        TerminalRenderer::print_newline();
    }

    // Auto-save session on exit
    if !messages.is_empty() {
        let s = if let Some(mut existing) = resumed_session.take() {
            // Preserve original metadata, update messages and timestamp
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

    TerminalRenderer::print_goodbye();
}

enum SlashResult {
    Continue,
    Exit,
    NotFound,
    /// Inject a user message into the conversation and process it with the LLM
    Review(String),
}

async fn handle_slash_command(
      input: &str,
      messages: &mut Vec<Message>,
      system_prompt: &str,
      context_size: usize,
      total_input: u64,
      total_output: u64,
      total_calls: u64,
      session_id: &str,
      cwd: &Path,
      pending_images: &PendingImages,
      resumed_session: Option<&Session>,
      allow_all: &mut bool,
      skills: &std::collections::HashMap<String, Skill>,
) -> SlashResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = *parts.first().unwrap_or(&"");

    // Helper closures for command matching (match passes &&str)
    let is_exit = |c: &&str| *c == format!("/{}", cmd::EXIT) || *c == format!("/{}", cmd::QUIT);
    let is_clear = |c: &&str| *c == format!("/{}", cmd::CLEAR);
    let is_compact = |c: &&str| *c == format!("/{}", cmd::COMPACT);
    let is_help = |c: &&str| *c == format!("/{}", cmd::HELP);
    let is_usage = |c: &&str| *c == format!("/{}", cmd::USAGE);

    match cmd {
        c if is_exit(&c) => SlashResult::Exit,
        c if is_clear(&c) => {
            messages.clear();
            pending_images.lock().unwrap().clear();
            println!("[conversation cleared]");
            SlashResult::Continue
        }
        c if is_compact(&c) => {
            let (compacted, was_compacted) = compact::compact_messages(messages, system_prompt, context_size);
            if was_compacted {
                let old_len = messages.len();
                *messages = compacted;
                println!("[compacted: {} → {} messages]", old_len, messages.len());
            } else {
                println!("[no compaction needed]");
            }
            SlashResult::Continue
        }
        c if is_help(&c) => {
            println!("{}", crate::render::StyledText::header("Available Commands"));
            println!("{}", crate::render::StyledText::separator());
            println!("  /help     - Show this help message");
            println!("  /exit     - Exit the agent");
            println!("  /quit     - Exit the agent (alias)");
            println!("  /clear    - Clear conversation history");
            println!("  /compact  - Manually compact conversation");
            println!("  /usage    - Show token usage statistics");
            println!("  /context  - Show context window usage");
            println!("  /save     - Save current session to disk");
            println!("  /sessions - List saved sessions");
            println!("  /commit   - Stage all changes and create a git commit");
            println!();
            println!("{}", crate::render::StyledText::header("Image Commands"));
            println!("{}", crate::render::StyledText::separator());
            println!("  /image <path>   - Add an image to the next message");
            println!("  /paste          - Read image from clipboard");
            println!("  /images         - Show pending images");
            println!("  /clear-images   - Clear pending images");
            println!();
            println!("{}", crate::render::StyledText::separator());
            println!("{}", crate::render::StyledText::info("Press Ctrl+C to interrupt current request"));
            SlashResult::Continue
        }
        c if is_usage(&c) => {
            println!("Usage this session:");
            println!("  API calls: {}", total_calls);
            println!("  Input:     {} tokens", total_input);
            println!("  Output:    {} tokens", total_output);
            println!("  Total:     {} tokens", total_input + total_output);
            SlashResult::Continue
        }
        "/context" => {
            let estimated = compact::estimate_messages_tokens(messages) + compact::estimate_tokens(system_prompt);
            let pct = estimated * 100 / context_size.max(1);
            println!("Context window: ~{} / {} tokens ({}%)", estimated, context_size, pct);
            println!("Messages: {}", messages.len());
            if pct > 80 {
                println!("[auto-compaction will trigger at 80%]");
            }
            SlashResult::Continue
        }
        "/save" => {
            // Use resumed_session if available, otherwise create new session
            let s: Session = if let Some(existing) = resumed_session {
                Session {
                    id: existing.id.clone(),
                    cwd: existing.cwd.clone(),
                    messages: messages.clone(),
                    created_at: existing.created_at.clone(),
                    updated_at: session::now_iso(),
                    metadata: existing.metadata.clone(),
                }
            } else {
                let mut s = Session::new(session_id.to_string(), cwd.to_string_lossy().to_string());
                s.messages = messages.clone();
                s.updated_at = session::now_iso();
                s
            };
            match session::save_session(&s).await {
                Ok(()) => println!("[session saved: {session_id}]"),
                Err(e) => eprintln!("error: {e}"),
            }
            SlashResult::Continue
        }
        "/sessions" => {
            let sessions = session::list_sessions().await;
            if sessions.is_empty() {
                println!("No saved sessions.");
            } else {
                println!("Saved sessions:");
                for (i, s) in sessions.iter().take(10).enumerate() {
                    let msg_count = s.messages.len();
                    println!("  {}. {} ({} msgs, {})", i + 1, s.id, msg_count, s.updated_at);
                }
                println!("\nResume with: aemeath --resume <session-id>");
            }
            SlashResult::Continue
        }
        "/commit" => {
            handle_commit(cwd).await;
            SlashResult::Continue
        }
        "/image" => {
            let path = parts.get(1);
            if path.is_none() {
                println!("Usage: /image <path>");
                println!("  Add an image file to the next message.");
                println!("  Supported formats: PNG, JPEG, GIF, WebP");
                return SlashResult::Continue;
            }
            let path = path.copied().unwrap_or("");
            // Resolve relative path
            let full_path = if Path::new(path).is_absolute() {
                path.to_string()
            } else {
                cwd.join(path).to_string_lossy().to_string()
            };

            match process_image_file(&full_path).await {
                Ok(img) => {
                    let size = img.original_size;
                    pending_images.lock().unwrap().push(img);
                    println!("[image added ({} bytes)]", size);
                    println!("  Type your message and press Enter to send with the image.");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                }
            }
            SlashResult::Continue
        }
        "/images" => {
            let images = pending_images.lock().unwrap();
            if images.is_empty() {
                println!("No pending images.");
            } else {
                println!("Pending images:");
                for (i, img) in images.iter().enumerate() {
                    println!("  {}. [image {}] ({} bytes)", i + 1, i + 1, img.final_size);
                }
                println!("\nImages will be sent with your next message.");
            }
            SlashResult::Continue
        }
        "/clear-images" => {
            let count = pending_images.lock().unwrap().len();
            pending_images.lock().unwrap().clear();
            println!("[cleared {} pending images]", count);
            SlashResult::Continue
        }
        "/paste" => {
            println!("[reading image from clipboard...]");
            match crate::image::read_clipboard_image().await {
                Ok(img) => {
                    println!(
                        "[clipboard image added ({} bytes)]",
                        img.final_size
                    );
                    pending_images.lock().unwrap().push(img);
                    println!("Image queued. Type your message to send it.");
                }
                Err(e) => {
                    eprintln!("error: {e}");
                }
            }
            SlashResult::Continue
        }
        // Try to execute via CommandRegistry
        _ => {
            let cmd_name = cmd.trim_start_matches('/');
            let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();

            // Try to find command in registry
            let registry = CommandRegistry::with_defaults();
            if let Some(cmd_obj) = registry.find(cmd_name) {
                let state = AppState::default();
                let config = aemeath_core::config::Config::default();
                let mut ctx = CommandContext::new(
                    Arc::new(state),
                    config,
                    cwd.to_string_lossy().to_string(),
                    session_id.to_string(),
                );

                match cmd_obj.execute(&args, &mut ctx).await {
                    CommandResult::Success(msg) => println!("{}", msg),
                    CommandResult::Error(msg) => eprintln!("error: {}", msg),
                    CommandResult::Action(action) => {
                        match action {
                            aemeath_core::command::CommandAction::Exit => return SlashResult::Exit,
                            aemeath_core::command::CommandAction::Clear => {
                                messages.clear();
                                println!("[cleared]");
                            }
                            aemeath_core::command::CommandAction::Review(prompt) => {
                                println!("[reviewing code changes...]");
                                return SlashResult::Review(prompt);
                            }
                            aemeath_core::command::CommandAction::ChangeMode(mode) => {
                                match mode.as_str() {
                                    "ask" => {
                                        *allow_all = false;
                                        println!("Permission mode set to: ask");
                                    }
                                    "auto-read" => {
                                        *allow_all = false;
                                        println!("Permission mode set to: auto-read");
                                    }
                                    "allow-all" => {
                                        *allow_all = true;
                                        println!("Permission mode set to: allow-all (warning: all tools will be auto-approved)");
                                    }
                                    _ => eprintln!("Unknown permission mode: {}", mode),
                                }
                            }
                            _ => println!("[action: {:?}]", action),
                        }
                    }
                    CommandResult::Confirm { message, .. } => {
                        println!("[confirm: {}]", message);
                    }
                }
                SlashResult::Continue
            } else if let Some(skill) = skills.values().find(|s| s.name == cmd_name || s.aliases.iter().any(|a| a == cmd_name)) {
                // Match skill alias — inject skill content as user message
                let args = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
                let mut content = skill.content.clone();
                if !args.is_empty() {
                    content = format!("{content}\n\nArguments: {args}");
                }
                println!("[skill: {}]", skill.name);
                SlashResult::Review(content)
            } else {
                SlashResult::NotFound
            }
        }
    }
}

/// Generate a human-friendly summary for a tool call input.
/// Falls back to the raw JSON string for unrecognized tools.
fn format_tool_summary(name: &str, input: &serde_json::Value) -> String {
    match name {
        "TodoRun" => {
            "execute all pending todos".to_string()
        }
        "TodoWrite" => {
            if let Some(todos) = input.get("todos").and_then(|t| t.as_array()) {
                let count = todos.len();
                let first = todos.first()
                    .and_then(|t| t.get("subject").and_then(|s| s.as_str()))
                    .unwrap_or("?");
                if count == 1 {
                    format!("{} todo: {}", count, first)
                } else if count <= 3 {
                    let subjects: Vec<&str> = todos.iter()
                        .filter_map(|t| t.get("subject").and_then(|s| s.as_str()))
                        .collect();
                    format!("{} todos: {}", count, subjects.join(", "))
                } else {
                    format!("{} todos: {}, ... +{} more", count, first, count - 1)
                }
            } else {
                input.to_string()
            }
        }
        _ => input.to_string(),
    }
}

fn ask_permission(tool_name: &str) -> bool {
    use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
    use crossterm::ExecutableCommand;
    use std::io::Write;

    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Yellow));
    let _ = stdout.execute(Print(format!("  Allow {tool_name}? [Y/n] ")));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.flush();

    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let answer = input.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

async fn handle_commit(cwd: &Path) {
    use tokio::process::Command;
    use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
    use crossterm::ExecutableCommand;
    use std::io::Write;

    // Check if git repo
    let is_git = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_git {
        eprintln!("not a git repository");
        return;
    }

    // Show diff stat
    let diff = Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(cwd)
        .output()
        .await;

    let status = Command::new("git")
        .args(["status", "--short"])
        .current_dir(cwd)
        .output()
        .await;

    if let Ok(output) = &status {
        let s = String::from_utf8_lossy(&output.stdout);
        if s.trim().is_empty() {
            println!("nothing to commit");
            return;
        }
        println!("Changes:");
        println!("{}", s.trim());
    }

    if let Ok(output) = &diff {
        let d = String::from_utf8_lossy(&output.stdout);
        if !d.trim().is_empty() {
            println!("\n{}", d.trim());
        }
    }

    // Ask for commit message
    let mut stdout = io::stdout();
    let _ = stdout.execute(SetForegroundColor(Color::Yellow));
    let _ = stdout.execute(Print("\nCommit message (empty to cancel): "));
    let _ = stdout.execute(ResetColor);
    let _ = stdout.flush();

    let mut msg = String::new();
    if io::stdin().read_line(&mut msg).is_err() || msg.trim().is_empty() {
        println!("[commit cancelled]");
        return;
    }
    let msg = msg.trim();

    // Stage all and commit
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(cwd)
        .output()
        .await;

    if let Err(e) = add {
        eprintln!("git add failed: {e}");
        return;
    }

    let commit = Command::new("git")
        .args(["commit", "-m", msg])
        .current_dir(cwd)
        .output()
        .await;

    match commit {
        Ok(output) => {
            let out = String::from_utf8_lossy(&output.stdout);
            if output.status.success() {
                let _ = stdout.execute(SetForegroundColor(Color::Green));
                println!("{}", out.trim());
                let _ = stdout.execute(ResetColor);
            } else {
                let err = String::from_utf8_lossy(&output.stderr);
                eprintln!("{}", err.trim());
            }
        }
        Err(e) => eprintln!("git commit failed: {e}"),
    }
}

/// Extract image file paths from user input text.
/// Returns (cleaned text with paths removed, list of processed images).
/// Recognizes patterns like:
///   - Bare paths: /path/to/image.png, ./screenshot.jpg
///   - Bracketed: [Image: /path/to/image.png]
///   - Tilde paths: ~/Desktop/photo.png
async fn extract_image_paths(input: &str, cwd: &Path) -> (String, Vec<ProcessedImage>) {
    let mut images = Vec::new();
    let mut clean = input.to_string();

    // Regex-free approach: split by whitespace, check each token
    let tokens: Vec<&str> = input.split_whitespace().collect();
    let mut paths_found: Vec<String> = Vec::new();

    for token in &tokens {
        // Strip surrounding brackets, quotes, parens
        let stripped = token
            .trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')' || c == '"' || c == '\'');

        if !is_image_file(stripped) {
            continue;
        }

        // Resolve the path
        let resolved = if stripped.starts_with('~') {
            if let Some(home) = dirs::home_dir() {
                home.join(&stripped[2..]).to_string_lossy().to_string()
            } else {
                continue;
            }
        } else if Path::new(stripped).is_absolute() {
            stripped.to_string()
        } else {
            cwd.join(stripped).to_string_lossy().to_string()
        };

        if !Path::new(&resolved).exists() {
            continue;
        }

        match process_image_file(&resolved).await {
            Ok(img) => {
                println!("[auto-attached image ({} bytes)]", img.original_size);
                images.push(img);
                paths_found.push(token.to_string());
            }
            Err(e) => {
                eprintln!("[warning: failed to load image: {}]", e);
            }
        }
    }

    // Remove found paths from the text
    for path in &paths_found {
        clean = clean.replace(path, "").trim().to_string();
    }

    // Clean up leftover bracket artifacts like "[]" or "[Image: ]"
    clean = clean
        .replace("[Image: ]", "")
        .replace("[Image:]", "")
        .replace("[]", "");
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");

    (clean, images)
}

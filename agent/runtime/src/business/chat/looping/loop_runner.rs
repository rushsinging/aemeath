use crate::api::agent::Agent;
use crate::api::agent_runner::{AgentRunOutcome, AgentRunStatus};
use crate::api::core::message::Message;
use crate::api::core::tool::{ToolContext, ToolRegistry};
use crate::api::provider::types::StopReason;
use crate::business::chat::looping::compact::auto_compact;
use crate::business::chat::looping::finalize::finalize_main_loop;
use crate::business::chat::looping::hook_ui::HookUi;
use crate::business::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::business::chat::looping::post_batch::run_post_tool_batch;
use crate::business::chat::looping::queue::append_queued_input;
use crate::business::chat::looping::stall::StallDetector;
use crate::business::chat::looping::task_reminder::TaskReminderState;
use crate::business::chat::looping::tool_context::{build_tool_context, ToolContextParts};
use crate::business::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::business::chat::looping::{
    ChatEventSink, QueueDrainPort, RuntimeStreamEvent, RuntimeStreamHandler,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub struct ChatLoopContext<S, Q>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
{
    pub sink: S,
    pub queue: Q,
    pub client: Arc<crate::api::provider::client::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<crate::api::provider::types::SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub workspace_context: Option<crate::api::session::WorkspaceContext>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<crate::api::core::tool::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn crate::api::core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<crate::api::core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: crate::api::hook::hook::HookRunner,
    pub memory_config: crate::api::core::config::MemoryConfig,
    pub json_logger: Option<Arc<std::sync::Mutex<logging::JsonLogger>>>,
}

/// Background task: runs the agent loop and sends UI events via sink.
pub async fn process_chat_loop<S, Q>(ctx: ChatLoopContext<S, Q>)
where
    S: ChatEventSink,
    Q: QueueDrainPort,
{
    let ChatLoopContext {
        sink,
        queue,
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        mut messages,
        context_size,
        cwd,
        workspace_context,
        session_id,
        read_files,
        session_reminders,
        agent_runner,
        allow_all,
        interrupted,
        cancel,
        task_store,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config,
        json_logger,
    } = ctx;
    let hook_ui = HookUi::new(sink.clone());

    let (cwd, working_root, path_base, context_stack) = if let Some(workspace) = workspace_context {
        (
            PathBuf::from(&workspace.path_base),
            Arc::new(Mutex::new(PathBuf::from(&workspace.working_root))),
            Arc::new(Mutex::new(PathBuf::from(&workspace.path_base))),
            Arc::new(Mutex::new(
                workspace
                    .context_stack
                    .into_iter()
                    .map(|entry| crate::api::core::tool::WorkingContext {
                        path_base: PathBuf::from(entry.path_base),
                        working_root: PathBuf::from(entry.working_root),
                    })
                    .collect(),
            )),
        )
    } else {
        let (cwd, working_root, path_base) = ToolContext::new_working_paths(cwd.clone());
        (
            cwd,
            working_root,
            path_base,
            Arc::new(Mutex::new(Vec::new())),
        )
    };
    hook_runner.set_project_dir(cwd.display().to_string());
    let agent = Agent {
        registry: &registry,
        ctx: build_tool_context(ToolContextParts {
            cwd: cwd.clone(),
            working_root,
            path_base,
            cancel: cancel.clone(),
            read_files: read_files.clone(),
            agent_runner: agent_runner.clone(),
            session_reminders: session_reminders.clone(),
            memory_config: memory_config.clone(),
            allow_all,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            session_id: session_id.clone(),
            context_stack,
        }),
    };

    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut task_reminder_state = TaskReminderState::new();
    let mut stall_detector = StallDetector::new();

    loop {
        turn_count += 1;
        sink.send_event(RuntimeStreamEvent::TurnChanged(turn_count))
            .await;
        // Refresh tool schemas each turn so dynamically registered MCP tools
        // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas();
        let tool_schema_tokens = crate::api::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            // Bug #49: drain queued input before handling cancellation,
            // so user-submitted messages are preserved even if interrupted.
            if append_queued_input(&queue, &sink, &mut messages).await {
                // User queued new input — resume with it instead of cancelling.
                sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                    .await;
                continue;
            }
            messages.truncate(messages_at_start);
            sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                .await;
            sink.send_event(RuntimeStreamEvent::Cancelled).await;
            if finalize_main_loop(
                &AgentRunOutcome {
                    status: AgentRunStatus::Cancelled,
                    turns: turn_count,
                    duration: turn_start.elapsed(),
                    role: None,
                    model: client.model_name().to_string(),
                },
                &sink,
                &hook_ui,
                &hook_runner,
                &session_id,
                &task_store,
            )
            .await
            .is_some()
            {
                continue;
            }
            break;
        }

        auto_compact(
            &sink,
            &hook_ui,
            &hook_runner,
            turn_count,
            &mut messages,
            &system_prompt_text,
            context_size,
            tool_schema_tokens,
            last_api_input_tokens,
        )
        .await;

        // Scan last assistant message for TaskCreate/TaskUpdate before building reminder
        task_reminder_state.update_from_messages(turn_count as u64, &messages);

        let messages_for_api: Vec<Message> = {
            let mut api_msgs = Vec::new();
            if !user_context.is_empty() {
                api_msgs.push(Message::user(format!(
                    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
                )));
            }
            // Inject task reminder if conditions are met
            if let Some(reminder) = task_reminder_state
                .build_reminder(turn_count as u64, &task_store)
                .await
            {
                api_msgs.push(reminder);
            }
            api_msgs.extend(messages.iter().cloned());
            api_msgs
        };

        let mut handler = RuntimeStreamHandler::new(sink.clone());

        log_llm_input(
            &json_logger,
            turn_count,
            client.model_name(),
            &messages_for_api,
            messages.len(),
            &system_blocks,
            &tool_schemas,
        );

        let api_start = std::time::Instant::now();
        let response = client
            .stream_message(
                &system_blocks,
                &messages_for_api,
                &tool_schemas,
                &mut handler,
                &cancel,
            )
            .await;
        let api_elapsed = api_start.elapsed().as_secs_f64();
        log::debug!(
            "turn api finished: session={}, turn={}, elapsed_secs={:.3}",
            session_id,
            turn_count,
            api_elapsed
        );
        match response {
            Ok(resp) => {
                last_api_input_tokens = resp.usage.input_tokens as u64;
                sink.send_event(RuntimeStreamEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                    last_input: resp.usage.input_tokens,
                    elapsed_secs: api_elapsed,
                })
                .await;

                messages.push(resp.assistant_message.clone());
                sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                    .await;

                if stall_detector.record_text(&resp.assistant_message.text_content()) {
                    sink.send_event(RuntimeStreamEvent::SystemMessage(
                        "[agent loop stopped: LLM is producing repetitive output]".to_string(),
                    ))
                    .await;
                    // Bug #49: drain queued input before breaking on stall.
                    if append_queued_input(&queue, &sink, &mut messages).await {
                        continue;
                    }
                    break;
                }

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                log_llm_output_and_tool_calls(
                    &json_logger,
                    turn_count,
                    client.provider_name(),
                    client.model_name(),
                    &resp,
                    &tool_calls,
                    api_elapsed,
                );
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    // Bug #49: drain queued user input before finishing the loop.
                    // If user submitted new messages while the last turn was running,
                    // consume them and continue instead of exiting.
                    if append_queued_input(&queue, &sink, &mut messages).await {
                        continue;
                    }
                    if let Some(text) = crate::business::chat::looping::reflection::run_reflection(
                        &memory_config,
                        turn_count,
                        &messages,
                        &cwd,
                        &client,
                        &system_prompt_text,
                    )
                    .await
                    {
                        sink.send_event(RuntimeStreamEvent::SystemMessage(text))
                            .await;
                    }
                    if let Some(outcome) = finalize_main_loop(
                        &AgentRunOutcome {
                            status: AgentRunStatus::Completed,
                            turns: turn_count,
                            duration: turn_start.elapsed(),
                            role: None,
                            model: client.model_name().to_string(),
                        },
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &session_id,
                        &task_store,
                    )
                    .await
                    {
                        messages.push(Message::user(format!(
                            "<system-reminder>\n{outcome}\n</system-reminder>"
                        )));
                        sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                            .await;
                        continue;
                    }
                    break;
                }
                {
                    let all_results = execute_tool_round(
                        &tool_calls,
                        &registry,
                        allow_all,
                        &agent,
                        &sink,
                        &hook_ui,
                        &hook_runner,
                        &json_logger,
                        turn_count,
                        client.model_name(),
                        max_agent_concurrency,
                        &interrupted,
                    )
                    .await;

                    // Build tool result message for API
                    messages.push(tool_results_for_api(all_results, &session_id)); // Sync after tool execution
                    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                        .await;

                    if append_queued_input(&queue, &sink, &mut messages).await {
                        continue;
                    }

                    run_post_tool_batch(&sink, &hook_ui, &hook_runner, &agent.ctx, turn_count)
                        .await;
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                sink.send_event(RuntimeStreamEvent::Error(error_msg.clone()))
                    .await;
                // Bug #49: drain queued input before handling API error.
                if append_queued_input(&queue, &sink, &mut messages).await {
                    continue;
                }
                if let Some(outcome) = finalize_main_loop(
                    &AgentRunOutcome {
                        status: AgentRunStatus::ApiError(error_msg),
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    },
                    &sink,
                    &hook_ui,
                    &hook_runner,
                    &session_id,
                    &task_store,
                )
                .await
                {
                    messages.push(Message::user(format!(
                        "<system-reminder>\n{outcome}\n</system-reminder>"
                    )));
                    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone()))
                        .await;
                    continue;
                }
                break;
            }
        }
    }
}

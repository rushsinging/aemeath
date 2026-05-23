mod agent_calls;
mod ask_user;
mod compact;
mod finalize;
mod handler;
mod hook_ui;
mod input_log;
mod non_agent;
mod permissions;
mod post_batch;
mod queue;
mod stall;
mod tools;

use crate::agent_runner::{AgentRunOutcome, AgentRunStatus};
use crate::tui::app::stream::compact::auto_compact;
use crate::tui::app::stream::finalize::finalize_main_loop;
use crate::tui::app::stream::handler::TuiStreamHandler;
use crate::tui::app::stream::hook_ui::HookUi;
pub(crate) use crate::tui::app::stream::input_log::logged_input_messages;
use crate::tui::app::stream::post_batch::run_post_tool_batch;
use crate::tui::app::stream::queue::append_queued_input;
use crate::tui::app::stream::stall::StallDetector;
use crate::tui::app::stream::tools::{execute_tool_round, tool_results_for_api};
use crate::tui::app::UiEvent;
use aemeath_core::agent::Agent;
use aemeath_core::message::Message;
use aemeath_core::tool::{ToolContext, ToolRegistry};
use aemeath_llm::types::StopReason;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::task_reminder::TaskReminderState;

/// Background task: runs the agent loop and sends UI events via channel
#[allow(clippy::too_many_arguments)]
pub async fn process_in_background(
    tx: mpsc::Sender<UiEvent>,
    queue_request_tx: mpsc::Sender<UiEvent>,
    client: Arc<aemeath_llm::client::LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<aemeath_llm::types::SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    mut messages: Vec<Message>,
    context_size: usize,
    cwd: PathBuf,
    session_id: String,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    session_reminders: Arc<std::sync::Mutex<aemeath_core::memory::SessionReminders>>,
    agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    allow_all: bool,
    interrupted: Arc<AtomicBool>,
    cancel: CancellationToken,
    task_store: Arc<aemeath_core::task::TaskStore>,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    hook_runner: aemeath_core::hook::HookRunner,
    memory_config: aemeath_core::config::MemoryConfig,
    json_logger: Option<Arc<std::sync::Mutex<aemeath_core::logging::JsonLogger>>>,
) {
    let hook_ui = HookUi::new(tx.clone());

    let (cwd, working_root, path_base) = ToolContext::new_working_paths(cwd.clone());
    hook_runner.set_project_dir(cwd.display().to_string());
    let ctx = ToolContext {
        cwd: cwd.clone(),
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
        max_agent_concurrency,
        agent_semaphore,
        progress_tx: None,
        parent_session_id: Some(session_id.clone()),
        context_stack: Arc::new(Mutex::new(Vec::new())),
    };
        let agent = Agent {
            registry: &registry,
            ctx,
        };

    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut task_reminder_state = TaskReminderState::new();
    let mut stall_detector = StallDetector::new();

    loop {
        turn_count += 1;
        crate::set_current_turn(turn_count);

        // Refresh tool schemas each turn so dynamically registered MCP tools
        // are visible to the LLM once the background connector finishes.
        let tool_schemas = registry.schemas();
        let tool_schema_tokens = aemeath_core::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            messages.truncate(messages_at_start);
            let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
            let _ = tx.send(UiEvent::Cancelled).await;
            if finalize_main_loop(
                &AgentRunOutcome {
                    status: AgentRunStatus::Cancelled,
                    turns: turn_count,
                    duration: turn_start.elapsed(),
                    role: None,
                    model: client.model_name().to_string(),
                },
                &tx,
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
            &tx,
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

        // Prepend CLAUDE.md user context for the API call
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

        let mut handler = TuiStreamHandler {
            tx: tx.clone(),
            first_text_time: None,
            total_chars: 0,
            last_tps_update: std::time::Instant::now(),
        };

        // JsonLogger: 记录 LLM 输入快照
        if let Some(ref jl) = json_logger {
            let new_msgs = logged_input_messages(&messages_for_api, messages.len());
            let sb_count = system_blocks.len();
            let sb_summary: Vec<serde_json::Value> = system_blocks
                .iter()
                .map(|sb| {
                    serde_json::json!({
                        "type": sb.block_type,
                        "len": sb.text.len(),
                    })
                })
                .collect();
            let schema_names: Vec<&str> = tool_schemas
                .iter()
                .map(|s| s.get("name").and_then(|v| v.as_str()).unwrap_or("?"))
                .collect();
            let data = serde_json::json!({
                "messages": new_msgs,
                "system_blocks_count": sb_count,
                "system_blocks": sb_summary,
                "tool_schemas_count": tool_schemas.len(),
                "tool_schemas_names": schema_names,
            });
            let _ = jl
                .lock()
                .unwrap()
                .log_input(turn_count, "default", client.model_name(), data);
        }

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
                let _ = tx
                    .send(UiEvent::Usage {
                        input: resp.usage.input_tokens,
                        output: resp.usage.output_tokens,
                        last_input: resp.usage.input_tokens,
                        elapsed_secs: api_elapsed,
                    })
                    .await;

                messages.push(resp.assistant_message.clone());
                let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;

                if stall_detector.record_text(&resp.assistant_message.text_content()) {
                    let _ = tx
                        .send(UiEvent::SystemMessage(
                            "[agent loop stopped: LLM is producing repetitive output]".to_string(),
                        ))
                        .await;
                    break;
                }

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                // JsonLogger: 记录 LLM 完整输出 + 工具调用
                if let Some(ref jl) = json_logger {
                    let blocks: Vec<serde_json::Value> = resp
                        .assistant_message
                        .content
                        .iter()
                        .filter_map(|block| serde_json::to_value(block).ok())
                        .collect();
                    let data = serde_json::json!({
                        "stop_reason": format!("{:?}", resp.stop_reason),
                        "input_tokens": resp.usage.input_tokens,
                        "output_tokens": resp.usage.output_tokens,
                        "elapsed_secs": api_elapsed,
                        "provider": client.provider_name(),
                        "content_blocks": blocks,
                    });
                    let _ = jl.lock().unwrap().log_output(
                        turn_count,
                        "default",
                        client.model_name(),
                        data,
                    );

                    for tc in &tool_calls {
                        let tc_data = serde_json::json!({
                            "tool_use_id": tc.id,
                            "tool_name": tc.name,
                            "input": tc.input,
                        });
                        let _ = jl.lock().unwrap().log_tool_call(
                            turn_count,
                            "default",
                            client.model_name(),
                            tc_data,
                        );
                    }
                }
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    // Bug #49: drain queued user input before finishing the loop.
                    // If user submitted new messages while the last turn was running,
                    // consume them and continue instead of exiting.
                    if append_queued_input(&queue_request_tx, &tx, &mut messages).await {
                        continue;
                    }
                    if let Some(text) = crate::reflection::run_reflection(
                        &memory_config,
                        turn_count,
                        &messages,
                        &cwd,
                        &client,
                        &system_prompt_text,
                    )
                    .await
                    {
                        let _ = tx.send(UiEvent::SystemMessage(text)).await;
                    }
                    if let Some(outcome) = finalize_main_loop(
                        &AgentRunOutcome {
                            status: AgentRunStatus::Completed,
                            turns: turn_count,
                            duration: turn_start.elapsed(),
                            role: None,
                            model: client.model_name().to_string(),
                        },
                        &tx,
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
                        let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
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
                        &tx,
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
                    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;

                    if append_queued_input(&queue_request_tx, &tx, &mut messages).await {
                        continue;
                    }

                    run_post_tool_batch(&tx, &hook_ui, &hook_runner, &agent.ctx, turn_count).await;
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                let _ = tx.send(UiEvent::Error(error_msg.clone())).await;
                if let Some(outcome) = finalize_main_loop(
                    &AgentRunOutcome {
                        status: AgentRunStatus::ApiError(error_msg),
                        turns: turn_count,
                        duration: turn_start.elapsed(),
                        role: None,
                        model: client.model_name().to_string(),
                    },
                    &tx,
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
                    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
                    continue;
                }
                break;
            }
        }
    }
}

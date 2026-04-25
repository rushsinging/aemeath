use crate::tui::app::UiEvent;
use aemeath_core::agent::{Agent, ToolCall};
use aemeath_core::message::Message;
use aemeath_core::tool::{ImageData, ToolContext, ToolRegistry};
use aemeath_llm::provider::StreamHandler;
use aemeath_llm::types::StopReason;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Background task: runs the agent loop and sends UI events via channel
#[allow(clippy::too_many_arguments)]
pub async fn process_in_background(
    tx: mpsc::Sender<UiEvent>,
    client: Arc<aemeath_llm::client::LlmClient>,
    registry: Arc<ToolRegistry>,
    system_blocks: Vec<aemeath_llm::types::SystemBlock>,
    system_prompt_text: String,
    user_context: String,
    mut messages: Vec<Message>,
    context_size: usize,
    cwd: PathBuf,
    #[allow(unused_variables)]
    session_id: String,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    agent_runner: Option<Arc<dyn aemeath_core::tool::AgentRunner>>,
    allow_all: bool,
    interrupted: Arc<AtomicBool>,
    cancel: CancellationToken,
    _task_store: Arc<aemeath_core::task::TaskStore>,
    max_tool_concurrency: usize,
    max_agent_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
) {
    _task_store.clear().await;

    let tool_schemas = registry.schemas();
    let tool_schema_tokens = aemeath_core::compact::estimate_tool_schemas_tokens(&tool_schemas);

    let ctx = ToolContext {
        cwd: cwd.clone(),
        cancel: cancel.clone(),
        read_files: read_files.clone(),
        agent_runner: agent_runner.clone(),
        plan_mode: None,
        allow_all,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
    };
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    const MAX_TURNS: usize = 100;
    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();

    for _ in 0..MAX_TURNS {
        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            messages.truncate(messages_at_start);
            let _ = tx.send(UiEvent::MessagesSync(messages)).await;
            let _ = tx.send(UiEvent::Cancelled).await;
            let _ = tx.send(UiEvent::Done).await;
            return;
        }

        struct TuiStreamHandler {
            tx: mpsc::Sender<UiEvent>,
            first_text_time: Option<std::time::Instant>,
            total_chars: usize,
            last_tps_update: std::time::Instant,
        }
        impl StreamHandler for TuiStreamHandler {
            fn on_text(&mut self, text: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::Text(text.to_string())) {
                    log::warn!("UI channel full, dropped Text event ({} bytes): {e}", text.len());
                }
                let now = std::time::Instant::now();
                if self.first_text_time.is_none() {
                    self.first_text_time = Some(now);
                    self.last_tps_update = now;
                }
                self.total_chars += text.len();
                // Update t/s every 200ms to avoid flooding
                if now.duration_since(self.last_tps_update).as_millis() >= 200 {
                    self.last_tps_update = now;
                    if let Some(start) = self.first_text_time {
                        let elapsed = now.duration_since(start).as_secs_f64();
                        if elapsed > 0.0 {
                            // Rough estimate: 1 token ≈ 4 chars for English, ~2 chars for Chinese.
                            // Use 3 as a middle ground.
                            let estimated_tokens = self.total_chars as f64 / 3.0;
                            let tps = estimated_tokens / elapsed;
                            let _ = self.tx.try_send(UiEvent::LiveTps(tps));
                        }
                    }
                }
            }
            fn on_tool_use_start(&mut self, name: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::ToolCallStart(name.to_string())) {
                    log::warn!("UI channel full, dropped ToolCallStart({name}): {e}");
                }
            }
            fn on_error(&mut self, error: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::SystemMessage(format!("[warn] {}", error))) {
                    log::warn!("UI channel full, dropped SystemMessage: {e}");
                }
            }
            fn on_text_block_complete(&mut self, text: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::TextBlockComplete(text.to_string())) {
                    log::warn!("UI channel full, dropped TextBlockComplete ({} bytes): {e}", text.len());
                }
            }
            fn on_thinking(&mut self, text: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::Thinking(text.to_string())) {
                    log::warn!("UI channel full, dropped Thinking event ({} bytes): {e}", text.len());
                }
            }
        }

        // Auto-compact if approaching context limit
        {
            use aemeath_core::compact;
            let should_compact = if last_api_input_tokens > 0 {
                compact::needs_compaction_actual(last_api_input_tokens, 0, context_size)
            } else {
                compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
            };
            if should_compact && messages.len() > 4 {
                let old_len = messages.len();
                compact::microcompact(&mut messages, 10);
                if compact::needs_compaction_full(&messages, &system_prompt_text, context_size, tool_schema_tokens)
                    || (last_api_input_tokens > 0 && compact::needs_compaction_actual(last_api_input_tokens, 0, context_size))
                {
                    let (compacted, was_compacted) = compact::compact_messages(&messages, &system_prompt_text, context_size);
                    if was_compacted {
                        messages = compacted;
                        let _ = tx.send(UiEvent::SystemMessage(
                            format!("[auto-compacted: {} → {} messages]", old_len, messages.len()),
                        )).await;
                    }
                }
            }
        }

        // Prepend CLAUDE.md user context for the API call
        let messages_for_api: Vec<Message> = {
            let mut api_msgs = Vec::new();
            if !user_context.is_empty() {
                api_msgs.push(Message::user(format!(
                    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
                )));
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
        let api_start = std::time::Instant::now();
        let response = client
            .stream_message(&system_blocks, &messages_for_api, &tool_schemas, &mut handler, &cancel)
            .await;
        let api_elapsed = api_start.elapsed().as_secs_f64();

        match response {
            Ok(resp) => {
                last_api_input_tokens = resp.usage.input_tokens as u64;
                let _ = tx.send(UiEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                    last_input: resp.usage.input_tokens,
                    elapsed_secs: api_elapsed,
                }).await;

                messages.push(resp.assistant_message.clone());
                let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    break;
                }

                {
                    let (approved, denied): (Vec<_>, Vec<_>) = if allow_all {
                        (tool_calls.iter().collect(), vec![])
                    } else {
                        tool_calls.iter().partition(|call| {
                            if call.name == "Bash" {
                                call.input.get("command")
                                    .and_then(|v| v.as_str())
                                    .map(aemeath_tools::bash::is_readonly_command)
                                    .unwrap_or(false)
                            } else {
                                registry.get(&call.name)
                                    .map(|t| t.is_read_only())
                                    .unwrap_or(false)
                            }
                        })
                    };

                    let mut denied_results: Vec<(String, String, bool, Vec<ImageData>)> = Vec::new();
                    for call in &denied {
                        let result = (
                            call.id.clone(),
                            format!("Tool {} denied: use --allow-all to permit write operations", call.name),
                            true,
                            Vec::new(),
                        );
                        denied_results.push(result.clone());
                        let _ = tx.send(UiEvent::ToolResult {
                            id: result.0,
                            tool_name: call.name.clone(),
                            output: result.1.clone(),
                            is_error: result.2,
                            images: result.3.clone(),
                        }).await;
                    }

                    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) = approved
                        .into_iter()
                        .partition(|c| c.name == "Agent");

                    let is_task_tool = |name: &str| name == "TaskCreate" || name == "TaskUpdate";

                    let non_agent_calls: Vec<ToolCall> = non_agent_approved.into_iter().map(|c| {
                        ToolCall { id: c.id.clone(), name: c.name.clone(), input: c.input.clone() }
                    }).collect();

                    for call in &non_agent_calls {
                        if !is_task_tool(&call.name) {
                            let _ = tx.send(UiEvent::ToolCall {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                summary: call.input.to_string(),
                            }).await;
                        }
                    }

                    use futures::future::join_all;
                    let per_tool_futures = non_agent_calls.iter().map(|call| {
                        let call = ToolCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            input: call.input.clone(),
                        };
                        let tx = tx.clone();
                        let agent_ref = &agent;
                        async move {
                            let skip_ui = is_task_tool(&call.name);
                            let results = agent_ref
                                .execute_tools(std::slice::from_ref(&call))
                                .await;
                            let mut collected = Vec::with_capacity(results.len());
                            for (id, output, is_error, images) in results {
                                if !skip_ui {
                                    let _ = tx.send(UiEvent::ToolResult {
                                        id: id.clone(),
                                        tool_name: call.name.clone(),
                                        output: output.clone(),
                                        is_error,
                                        images: images.clone(),
                                    }).await;
                                }
                                collected.push((id, output, is_error, images));
                            }
                            collected
                        }
                    });
                    let collected_per_tool: Vec<Vec<(String, String, bool, Vec<ImageData>)>> =
                        join_all(per_tool_futures).await;
                    let non_agent_results: Vec<(String, String, bool, Vec<ImageData>)> =
                        collected_per_tool.into_iter().flatten().collect();

                    let mut agent_results: Vec<(String, String, bool, Vec<ImageData>)> = Vec::new();
                    let batch_size = max_agent_concurrency.max(1);

                    let call_to_task: std::collections::HashMap<String, String> = agent_approved
                        .iter()
                        .filter_map(|c| {
                            c.input.get("taskId")
                                .and_then(|v| v.as_str())
                                .map(|t| (c.id.clone(), t.to_string()))
                        })
                        .collect();
                    for tid in call_to_task.values() {
                        let _ = tx.send(UiEvent::ToolResult {
                            id: tid.clone(),
                            tool_name: "TaskUpdate".to_string(),
                            output: "reset to pending".to_string(),
                            is_error: false,
                            images: Vec::new(),
                        }).await;
                    }

                    // Process agent calls in batches with semaphore
                    for batch in agent_approved.chunks(batch_size) {
                        if interrupted.load(Ordering::Relaxed) { break; }
                        let agent_futures: Vec<_> = batch.iter().map(|call| {
                            let call = ToolCall {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                input: call.input.clone(),
                            };
                            let tx = tx.clone();
                            let agent_ref = &agent;
                            async move {
                                let results = agent_ref.execute_tools(std::slice::from_ref(&call)).await;
                                for (id, output, is_error, images) in &results {
                                    let _ = tx.send(UiEvent::ToolResult {
                                        id: id.clone(),
                                        tool_name: call.name.clone(),
                                        output: output.clone(),
                                        is_error: *is_error,
                                        images: images.clone(),
                                    }).await;
                                }
                                results
                            }
                        }).collect();
                        let batch_results: Vec<Vec<(String, String, bool, Vec<ImageData>)>> =
                            futures::future::join_all(agent_futures).await;
                        for r in batch_results.into_iter().flatten() {
                            agent_results.push(r);
                        }
                    }

                    let all_results: Vec<(String, String, bool, Vec<ImageData>)> = non_agent_results
                        .into_iter()
                        .chain(agent_results.into_iter())
                        .chain(denied_results.into_iter())
                        .collect();

                    // Build tool result message for API
                    messages.push(Message::tool_results_rich(all_results));

                    // Sync after tool execution
                    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
                }
            }
            Err(e) => {
                let _ = tx.send(UiEvent::Error(e.to_string())).await;
                let _ = tx.send(UiEvent::Done).await;
                return;
            }
        }
    }

    messages.truncate(messages_at_start);
    let _ = tx.send(UiEvent::DoneWithDuration(turn_start.elapsed())).await;
}

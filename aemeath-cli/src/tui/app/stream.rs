use crate::tui::app::UiEvent;
use aemeath_core::agent::{Agent, ToolCall};
use aemeath_core::config::hooks::HookEvent;
use aemeath_core::hook::{
    CompactHookData, HookData, HookJsonOutput, HookResult, StopHookData, ToolHookData,
};
use aemeath_core::message::Message;
use aemeath_core::tool::{ImageData, ToolContext, ToolRegistry};
use aemeath_llm::provider::StreamHandler;
use aemeath_llm::types::StopReason;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::task_reminder::TaskReminderState;

pub(crate) fn logged_input_messages(
    messages_for_api: &[Message],
    persisted_message_count: usize,
) -> Vec<serde_json::Value> {
    let injected_count = messages_for_api
        .len()
        .saturating_sub(persisted_message_count);
    let mut indices: Vec<usize> = (0..injected_count).collect();
    if persisted_message_count > 0 && !messages_for_api.is_empty() {
        indices.push(messages_for_api.len() - 1);
    }
    indices
        .into_iter()
        .filter_map(|index| messages_for_api.get(index))
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
                "len": m.content.len(),
            })
        })
        .collect()
}

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

    let tool_schemas = registry.schemas();
    let tool_schema_tokens = aemeath_core::compact::estimate_tool_schemas_tokens(&tool_schemas);

    let ctx = ToolContext {
        cwd: cwd.clone(),
        cancel: cancel.clone(),
        read_files: read_files.clone(),
        agent_runner: agent_runner.clone(),
        session_reminders: Some(session_reminders.clone()),
        plan_mode: None,
        allow_all,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        progress_tx: None,
        parent_session_id: Some(session_id.clone()),
    };
    let agent = Agent {
        registry: &registry,
        ctx,
    };

    const MAX_TURNS: usize = 100;
    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;
    let mut task_reminder_state = TaskReminderState::new();

    for _ in 0..MAX_TURNS {
        turn_count += 1;
        crate::set_current_turn(turn_count);
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
                    log::warn!(
                        "UI channel full, dropped Text event ({} bytes): {e}",
                        text.len()
                    );
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
                if let Err(e) = self
                    .tx
                    .try_send(UiEvent::SystemMessage(format!("[warn] {}", error)))
                {
                    log::warn!("UI channel full, dropped SystemMessage: {e}");
                }
            }
            fn on_text_block_complete(&mut self, text: &str) {
                if let Err(e) = self
                    .tx
                    .try_send(UiEvent::TextBlockComplete(text.to_string()))
                {
                    log::warn!(
                        "UI channel full, dropped TextBlockComplete ({} bytes): {e}",
                        text.len()
                    );
                }
            }
            fn on_thinking(&mut self, text: &str) {
                if let Err(e) = self.tx.try_send(UiEvent::Thinking(text.to_string())) {
                    log::warn!(
                        "UI channel full, dropped Thinking event ({} bytes): {e}",
                        text.len()
                    );
                }
            }
        }

        // Auto-compact if approaching context limit
        {
            use aemeath_core::compact;

            // PreCompact hook: 在压缩前触发，可阻止压缩
            let pre_compact_results = hook_ui
                .run_json(
                    &hook_runner,
                    HookEvent::PreCompact,
                    None,
                    HookData::Compact(CompactHookData {
                        turns: turn_count,
                        messages_before: messages.len(),
                        messages_after: None,
                        was_compacted: false,
                    }),
                )
                .await;
            let pre_compact_blocked = pre_compact_results.iter().any(|(_, result, json)| {
                result.blocked
                    || json
                        .as_ref()
                        .is_some_and(|j| j.decision.as_deref() == Some("block"))
            });
            for (_entry, _result, json_output) in &pre_compact_results {
                if let Some(json) = json_output {
                    if let Some(ref ctx) = json.additional_context {
                        let _ = tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                    }
                    if let Some(ref msg) = json.system_message {
                        let _ = tx.send(UiEvent::SystemMessage(msg.clone())).await;
                    }
                }
            }

            if pre_compact_blocked {
                log::warn!("PreCompact hook blocked compaction");
            } else {
                let should_compact = if last_api_input_tokens > 0 {
                    compact::needs_compaction_actual(last_api_input_tokens, 0, context_size)
                } else {
                    compact::needs_compaction_full(
                        &messages,
                        &system_prompt_text,
                        context_size,
                        tool_schema_tokens,
                    )
                };
                if should_compact && messages.len() > 4 {
                    let old_len = messages.len();
                    compact::microcompact(&mut messages, 10);
                    if compact::needs_compaction_full(
                        &messages,
                        &system_prompt_text,
                        context_size,
                        tool_schema_tokens,
                    ) || (last_api_input_tokens > 0
                        && compact::needs_compaction_actual(last_api_input_tokens, 0, context_size))
                    {
                        let (compacted, was_compacted) =
                            compact::compact_messages(&messages, &system_prompt_text, context_size);
                        if was_compacted {
                            let new_len = compacted.len();
                            messages = compacted;
                            let _ = tx
                                .send(UiEvent::SystemMessage(format!(
                                    "[auto-compacted: {} → {} messages]",
                                    old_len, new_len
                                )))
                                .await;

                            // PostCompact hook: 在压缩成功后触发
                            let post_compact_results = hook_ui
                                .run_json(
                                    &hook_runner,
                                    HookEvent::PostCompact,
                                    None,
                                    HookData::Compact(CompactHookData {
                                        turns: turn_count,
                                        messages_before: old_len,
                                        messages_after: Some(new_len),
                                        was_compacted: true,
                                    }),
                                )
                                .await;
                            for (_entry, _result, json_output) in &post_compact_results {
                                if let Some(json) = json_output {
                                    if let Some(ref ctx) = json.additional_context {
                                        let _ = tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                                    }
                                    if let Some(ref msg) = json.system_message {
                                        let _ = tx.send(UiEvent::SystemMessage(msg.clone())).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

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
                    break;
                }
                {
                    let (approved, denied): (Vec<_>, Vec<_>) = if allow_all {
                        (tool_calls.iter().collect(), vec![])
                    } else {
                        tool_calls.iter().partition(|call| {
                            if call.name == "Bash" {
                                call.input
                                    .get("command")
                                    .and_then(|v| v.as_str())
                                    .map(aemeath_tools::bash::is_readonly_command)
                                    .unwrap_or(false)
                            } else {
                                registry
                                    .get(&call.name)
                                    .map(|t| t.is_read_only())
                                    .unwrap_or(false)
                            }
                        })
                    };

                    let mut denied_results: Vec<(String, String, bool, Vec<ImageData>)> =
                        Vec::new();
                    for call in &denied {
                        // PermissionDenied hook: notify when a tool is denied
                        let _hook_results = hook_ui
                            .run_plain(
                                &hook_runner,
                                HookEvent::PermissionDenied,
                                Some(&call.name),
                                HookData::Permission(aemeath_core::hook::PermissionHookData {
                                    tool_name: call.name.clone(),
                                    permission_rule: "deny".to_string(),
                                }),
                            )
                            .await;

                        let result = (
                            call.id.clone(),
                            format!(
                                "Tool {} denied: use --allow-all to permit write operations",
                                call.name
                            ),
                            true,
                            Vec::new(),
                        );
                        denied_results.push(result.clone());
                        let _ = tx
                            .send(UiEvent::ToolResult {
                                id: result.0,
                                tool_name: call.name.clone(),
                                output: result.1.clone(),
                                is_error: result.2,
                                images: result.3.clone(),
                            })
                            .await;
                    }

                    let (agent_approved, non_agent_approved): (Vec<_>, Vec<_>) =
                        approved.into_iter().partition(|c| c.name == "Agent");

                    let is_ask_user = |name: &str| name == "AskUserQuestion";

                    let non_agent_calls: Vec<ToolCall> = non_agent_approved
                        .into_iter()
                        .map(|c| ToolCall {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            input: c.input.clone(),
                        })
                        .collect();

                    // 拦截 AskUserQuestion：不走 execute_tools，而是通过 UI 询问用户
                    log::debug!(
                        "[AskUser] non_agent_calls: {:?}",
                        non_agent_calls.iter().map(|c| &c.name).collect::<Vec<_>>()
                    );
                    let mut ask_user_results: Vec<(String, String, bool, Vec<ImageData>)> =
                        Vec::new();
                    let ask_calls: Vec<&ToolCall> = non_agent_calls
                        .iter()
                        .filter(|c| is_ask_user(&c.name))
                        .collect();
                    log::debug!("[AskUser] ask_calls count: {}", ask_calls.len());
                    for call in &ask_calls {
                        // PermissionRequest hook: notify before executing AskUserQuestion tool
                        let _hook_results = hook_ui
                            .run_plain(
                                &hook_runner,
                                HookEvent::PermissionRequest,
                                Some(&call.name),
                                HookData::Permission(aemeath_core::hook::PermissionHookData {
                                    tool_name: call.name.clone(),
                                    permission_rule: "manual".to_string(),
                                }),
                            )
                            .await;

                        let question = call
                            .input
                            .get("question")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let options: Vec<String> = call
                            .input
                            .get("options")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let allow_free_input = call
                            .input
                            .get("allow_free_input")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true);
                        let multi_select = call
                            .input
                            .get("multi_select")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let default = call
                            .input
                            .get("default")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());

                        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<String>();
                        let _ = tx
                            .send(UiEvent::AskUser {
                                id: call.id.clone(),
                                question: question.clone(),
                                options: options.clone(),
                                allow_free_input,
                                multi_select,
                                default: default.clone(),
                                reply_tx,
                            })
                            .await;

                        // 挂起等待用户回答
                        let answer = match reply_rx.await {
                            Ok(a) if !a.is_empty() => a,
                            _ => default.unwrap_or_default(),
                        };
                        let _ = tx
                            .send(UiEvent::ToolResult {
                                id: call.id.clone(),
                                tool_name: call.name.clone(),
                                output: answer.clone(),
                                is_error: false,
                                images: Vec::new(),
                            })
                            .await;
                        ask_user_results.push((call.id.clone(), answer, false, Vec::new()));
                    }

                    // 其他 non-agent tool calls（排除 AskUserQuestion）
                    let other_calls: Vec<&ToolCall> = non_agent_calls
                        .iter()
                        .filter(|c| !is_ask_user(&c.name))
                        .collect();

                    for call in &other_calls {
                        let _ = tx
                            .send(UiEvent::ToolCall {
                                id: call.id.clone(),
                                name: call.name.clone(),
                                summary: call.input.to_string(),
                            })
                            .await;
                    }

                    // Execute tool calls sequentially so each ToolResult is sent
                    // immediately after completion (instead of join_all batching).
                    let mut non_agent_results: Vec<(String, String, bool, Vec<ImageData>)> =
                        Vec::new();
                    for call in &other_calls {
                        // PermissionRequest hook: notify before executing non-agent tool
                        let _hook_results = hook_ui
                            .run_plain(
                                &hook_runner,
                                HookEvent::PermissionRequest,
                                Some(&call.name),
                                HookData::Permission(aemeath_core::hook::PermissionHookData {
                                    tool_name: call.name.clone(),
                                    permission_rule: "auto".to_string(),
                                }),
                            )
                            .await;

                        let call = ToolCall {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            input: call.input.clone(),
                        };
                        // PreToolUse hook: 检查是否应阻止执行
                        let pre_results = hook_ui
                            .run_plain(
                                &hook_runner,
                                HookEvent::PreToolUse,
                                Some(&call.name),
                                HookData::Tool(ToolHookData {
                                    tool_name: call.name.clone(),
                                    tool_input: call.input.clone(),
                                    tool_output: None,
                                    is_error: None,
                                }),
                            )
                            .await;
                        let blocked = pre_results.iter().any(|r| r.blocked);
                        if blocked {
                            let _ = tx
                                .send(UiEvent::ToolResult {
                                    id: call.id.clone(),
                                    tool_name: call.name.clone(),
                                    output: format!("Blocked by PreToolUse hook"),
                                    is_error: true,
                                    images: Vec::new(),
                                })
                                .await;
                            non_agent_results.push((
                                call.id.clone(),
                                "Blocked by PreToolUse hook".to_string(),
                                true,
                                Vec::new(),
                            ));
                            continue;
                        }

                        let results = agent.execute_tools(std::slice::from_ref(&call)).await;
                        for (id, output, is_error, images) in results {
                            // JsonLogger: 记录工具执行结果（完整输出）
                            if let Some(ref jl) = json_logger {
                                let tr_data = serde_json::json!({
                                    "tool_use_id": id,
                                    "tool_name": call.name,
                                    "is_error": is_error,
                                    "output": output,
                                });
                                let _ = jl.lock().unwrap().log_tool_result(
                                    turn_count,
                                    "default",
                                    client.model_name(),
                                    tr_data,
                                );
                            }
                            // PostToolUse hook: run with JSON output parsing
                            let hook_results = hook_ui
                                .run_json(
                                    &hook_runner,
                                    HookEvent::PostToolUse,
                                    Some(&call.name),
                                    HookData::Tool(ToolHookData {
                                        tool_name: call.name.clone(),
                                        tool_input: call.input.clone(),
                                        tool_output: Some(output.clone()),
                                        is_error: Some(is_error),
                                    }),
                                )
                                .await;
                            for (_entry, _result, json_output) in &hook_results {
                                if let Some(json) = json_output {
                                    if let Some(ref ctx) = json.additional_context {
                                        let _ = tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                                    }
                                    if let Some(ref msg) = json.system_message {
                                        let _ = tx.send(UiEvent::SystemMessage(msg.clone())).await;
                                    }
                                }
                            }

                            // PostToolUseFailure hook: 工具执行失败时触发
                            if is_error {
                                let hook_results = hook_ui
                                    .run_json(
                                        &hook_runner,
                                        HookEvent::PostToolUseFailure,
                                        Some(&call.name),
                                        HookData::Tool(ToolHookData {
                                            tool_name: call.name.clone(),
                                            tool_input: call.input.clone(),
                                            tool_output: Some(output.clone()),
                                            is_error: Some(is_error),
                                        }),
                                    )
                                    .await;
                                for (_entry, _result, json_output) in &hook_results {
                                    if let Some(json) = json_output {
                                        if let Some(ref ctx) = json.additional_context {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                                        }
                                        if let Some(ref msg) = json.system_message {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(msg.clone())).await;
                                        }
                                    }
                                }
                            }

                            // TaskCreated hook: TaskCreate 工具执行成功时触发
                            if !is_error && call.name == "TaskCreate" {
                                let hook_results = hook_ui
                                    .run_json(
                                        &hook_runner,
                                        HookEvent::TaskCreated,
                                        None,
                                        HookData::Tool(ToolHookData {
                                            tool_name: "TaskCreate".to_string(),
                                            tool_input: call.input.clone(),
                                            tool_output: Some(output.clone()),
                                            is_error: Some(false),
                                        }),
                                    )
                                    .await;
                                for (_entry, _result, json_output) in &hook_results {
                                    if let Some(json) = json_output {
                                        if let Some(ref ctx) = json.additional_context {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                                        }
                                        if let Some(ref msg) = json.system_message {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(msg.clone())).await;
                                        }
                                    }
                                }
                            }

                            // TaskCompleted hook: TaskUpdate 将任务标记为 completed 时触发
                            if !is_error
                                && call.name == "TaskUpdate"
                                && output.contains("Status: Completed")
                            {
                                let hook_results = hook_ui
                                    .run_json(
                                        &hook_runner,
                                        HookEvent::TaskCompleted,
                                        None,
                                        HookData::Tool(ToolHookData {
                                            tool_name: "TaskUpdate".to_string(),
                                            tool_input: call.input.clone(),
                                            tool_output: Some(output.clone()),
                                            is_error: Some(false),
                                        }),
                                    )
                                    .await;
                                for (_entry, _result, json_output) in &hook_results {
                                    if let Some(json) = json_output {
                                        if let Some(ref ctx) = json.additional_context {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                                        }
                                        if let Some(ref msg) = json.system_message {
                                            let _ =
                                                tx.send(UiEvent::SystemMessage(msg.clone())).await;
                                        }
                                    }
                                }
                            }

                            let _ = tx
                                .send(UiEvent::ToolResult {
                                    id: id.clone(),
                                    tool_name: call.name.clone(),
                                    output: output.clone(),
                                    is_error,
                                    images: images.clone(),
                                })
                                .await;
                            non_agent_results.push((id, output, is_error, images));
                        }
                    }

                    let mut agent_results: Vec<(String, String, bool, Vec<ImageData>)> = Vec::new();
                    let batch_size = max_agent_concurrency.max(1);

                    let call_to_task: std::collections::HashMap<String, String> = agent_approved
                        .iter()
                        .filter_map(|c| {
                            c.input
                                .get("taskId")
                                .and_then(|v| v.as_str())
                                .map(|t| (c.id.clone(), t.to_string()))
                        })
                        .collect();
                    for tid in call_to_task.values() {
                        let _ = tx
                            .send(UiEvent::ToolResult {
                                id: tid.clone(),
                                tool_name: "TaskUpdate".to_string(),
                                output: "reset to pending".to_string(),
                                is_error: false,
                                images: Vec::new(),
                            })
                            .await;
                    }

                    // Process agent calls in batches with semaphore
                    for batch in agent_approved.chunks(batch_size) {
                        if interrupted.load(Ordering::Relaxed) {
                            break;
                        }

                        // Send ToolCall event for each Agent call so the TUI can
                        // display role/model/description info (replacing the bare "● Agent...")
                        for call in batch {
                            let _ = tx
                                .send(UiEvent::ToolCall {
                                    id: call.id.clone(),
                                    name: call.name.clone(),
                                    summary: call.input.to_string(),
                                })
                                .await;
                        }

                        let agent_futures: Vec<_> = batch
                            .iter()
                            .map(|call| {
                                let call = ToolCall {
                                    id: call.id.clone(),
                                    name: call.name.clone(),
                                    input: call.input.clone(),
                                };
                                let tx = tx.clone();
                                let hook_ui = hook_ui.clone();
                                let mut ag_ctx = agent.ctx.clone();
                                let hook_runner = hook_runner.clone();
                                let registry_ref = registry.clone();
                                async move {
                                    // PreToolUse hook for Agent calls
                                    let pre_results = hook_ui
                                        .run_plain(
                                            &hook_runner,
                                            HookEvent::PreToolUse,
                                            Some(&call.name),
                                            HookData::Tool(ToolHookData {
                                                tool_name: call.name.clone(),
                                                tool_input: call.input.clone(),
                                                tool_output: None,
                                                is_error: None,
                                            }),
                                        )
                                        .await;
                                    let blocked = pre_results.iter().any(|r| r.blocked);
                                    if blocked {
                                        let _ = tx
                                            .send(UiEvent::ToolResult {
                                                id: call.id.clone(),
                                                tool_name: call.name.clone(),
                                                output: "Blocked by PreToolUse hook".to_string(),
                                                is_error: true,
                                                images: Vec::new(),
                                            })
                                            .await;
                                        return vec![(
                                            call.id.clone(),
                                            "Blocked by PreToolUse hook".to_string(),
                                            true,
                                            Vec::new(),
                                        )];
                                    }

                                    // Set up progress channel so CliAgentRunner can stream
                                    // per-turn output back to the TUI while the sub-agent runs.
                                    let (prog_tx, mut prog_rx) =
                                        tokio::sync::mpsc::channel::<
                                            aemeath_core::tool::AgentProgressEvent,
                                        >(32);
                                    ag_ctx.progress_tx = Some(prog_tx);

                                    let call_id = call.id.clone();
                                    let ui_tx = tx.clone();
                                    // Spawn a task that forwards progress to the UI
                                    let forward_handle = tokio::spawn(async move {
                                        while let Some(event) = prog_rx.recv().await {
                                            let _ = ui_tx
                                                .send(UiEvent::AgentProgress {
                                                    tool_id: call_id.clone(),
                                                    event,
                                                })
                                                .await;
                                        }
                                    });

                                    // Call AgentTool directly via registry with progress_tx enabled ctx
                                    let agent_tool = registry_ref
                                        .get("Agent")
                                        .expect("Agent tool not found in registry");
                                    let result = agent_tool.call(call.input.clone(), &ag_ctx).await;
                                    let results = vec![(
                                        call.id.clone(),
                                        result.output,
                                        result.is_error,
                                        result.images,
                                    )];

                                    // Drop ag_ctx to close channel, signal the forward task
                                    drop(ag_ctx);
                                    // Wait briefly for forward task to flush remaining messages
                                    let _ = tokio::time::timeout(
                                        std::time::Duration::from_millis(500),
                                        forward_handle,
                                    )
                                    .await;

                                    for (id, output, is_error, images) in &results {
                                        // PostToolUse hook for Agent calls
                                        let _ = hook_ui
                                            .run_json(
                                                &hook_runner,
                                                HookEvent::PostToolUse,
                                                Some(&call.name),
                                                HookData::Tool(ToolHookData {
                                                    tool_name: call.name.clone(),
                                                    tool_input: call.input.clone(),
                                                    tool_output: Some(output.clone()),
                                                    is_error: Some(*is_error),
                                                }),
                                            )
                                            .await;

                                        // PostToolUseFailure hook: Agent 工具执行失败时触发
                                        if *is_error {
                                            let hook_results = hook_ui
                                                .run_json(
                                                    &hook_runner,
                                                    HookEvent::PostToolUseFailure,
                                                    Some(&call.name),
                                                    HookData::Tool(ToolHookData {
                                                        tool_name: call.name.clone(),
                                                        tool_input: call.input.clone(),
                                                        tool_output: Some(output.clone()),
                                                        is_error: Some(*is_error),
                                                    }),
                                                )
                                                .await;
                                            for (_entry, _result, json_output) in &hook_results {
                                                if let Some(json) = json_output {
                                                    if let Some(ref ctx) = json.additional_context {
                                                        let _ = tx
                                                            .send(UiEvent::SystemMessage(
                                                                ctx.clone(),
                                                            ))
                                                            .await;
                                                    }
                                                    if let Some(ref msg) = json.system_message {
                                                        let _ = tx
                                                            .send(UiEvent::SystemMessage(
                                                                msg.clone(),
                                                            ))
                                                            .await;
                                                    }
                                                }
                                            }
                                        }

                                        let _ = tx
                                            .send(UiEvent::ToolResult {
                                                id: id.clone(),
                                                tool_name: call.name.clone(),
                                                output: output.clone(),
                                                is_error: *is_error,
                                                images: images.clone(),
                                            })
                                            .await;
                                    }
                                    results
                                }
                            })
                            .collect();
                        let batch_results: Vec<Vec<(String, String, bool, Vec<ImageData>)>> =
                            futures::future::join_all(agent_futures).await;
                        for r in batch_results.into_iter().flatten() {
                            agent_results.push(r);
                        }
                    }

                    let all_results: Vec<(String, String, bool, Vec<ImageData>)> = ask_user_results
                        .into_iter()
                        .chain(non_agent_results.into_iter())
                        .chain(agent_results.into_iter())
                        .chain(denied_results.into_iter())
                        .collect();

                    // Build tool result message for API
                    messages.push(Message::tool_results_rich(all_results));
                    // Sync after tool execution
                    let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;

                    if let Some(queued) = drain_queued_input(&queue_request_tx).await {
                        for input in queued {
                            messages.push(Message::user(input));
                        }
                        let _ = tx.send(UiEvent::MessagesSync(messages.clone())).await;
                    }

                    // PostToolBatch hook: 批量工具调用完成后触发（汇总注入）
                    let post_batch_results = hook_ui
                        .run_json(
                            &hook_runner,
                            HookEvent::PostToolBatch,
                            None,
                            HookData::Stop(StopHookData { turns: turn_count }),
                        )
                        .await;
                    for (_entry, _result, json_output) in &post_batch_results {
                        if let Some(json) = json_output {
                            if let Some(ref ctx) = json.additional_context {
                                let _ = tx.send(UiEvent::SystemMessage(ctx.clone())).await;
                            }
                            if let Some(ref msg) = json.system_message {
                                let _ = tx.send(UiEvent::SystemMessage(msg.clone())).await;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(UiEvent::Error(e.to_string())).await;
                // StopFailure hook: API 错误导致 agent 循环结束
                let stop_results = hook_ui
                    .run_json(
                        &hook_runner,
                        HookEvent::StopFailure,
                        None,
                        HookData::Stop(StopHookData { turns: turn_count }),
                    )
                    .await;
                let (system_message, additional_context) = stop_results
                    .into_iter()
                    .find_map(|(_, _, json_output)| json_output)
                    .map(|output| (output.system_message, output.additional_context))
                    .unwrap_or((None, None));
                let _ = tx
                    .send(UiEvent::StopFailureHook {
                        system_message,
                        additional_context,
                    })
                    .await;
                let _ = tx.send(UiEvent::Done).await;
                return;
            }
        }
    }

    messages.truncate(messages_at_start);

    // Stop hook: agent 循环结束
    let _ = hook_ui
        .run_plain(
            &hook_runner,
            HookEvent::Stop,
            None,
            HookData::Stop(StopHookData { turns: turn_count }),
        )
        .await;

    let _ = tx
        .send(UiEvent::DoneWithDuration(turn_start.elapsed()))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logged_input_messages_happy_path_includes_latest_user_message() {
        let messages = vec![Message::user("context"), Message::user("hello")];

        let logged = logged_input_messages(&messages, 1);

        assert_eq!(logged.len(), 2);
        assert!(logged[0]["content"].to_string().contains("context"));
        assert!(logged[1]["content"].to_string().contains("hello"));
    }

    #[test]
    fn test_logged_input_messages_boundary_no_injected_message() {
        let messages = vec![Message::user("hello")];

        let logged = logged_input_messages(&messages, 1);

        assert_eq!(logged.len(), 1);
        assert!(logged[0]["content"].to_string().contains("hello"));
    }

    #[test]
    fn test_logged_input_messages_error_empty_input_is_empty() {
        let logged = logged_input_messages(&[], 0);

        assert!(logged.is_empty());
    }
}

#[derive(Clone)]
struct HookUi {
    tx: mpsc::Sender<UiEvent>,
}

impl HookUi {
    fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }

    async fn run_json(
        &self,
        runner: &aemeath_core::hook::HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<(
        aemeath_core::config::hooks::HookEntry,
        HookResult,
        Option<HookJsonOutput>,
    )> {
        let hooks = runner.matching_hooks(event, tool_name);
        if hooks.is_empty() {
            return Vec::new();
        }

        let command = hooks
            .first()
            .map(|hook| hook.command.clone())
            .unwrap_or_default();
        let event_name = hook_event_name(event);
        let _ = self
            .tx
            .send(UiEvent::HookStart {
                event: event_name.to_string(),
                command,
            })
            .await;

        let hook_results = runner.run_hooks_with_json(event, tool_name, data).await;

        for (_, result, _) in &hook_results {
            let _ = self
                .tx
                .send(UiEvent::HookEnd {
                    event: event_name.to_string(),
                    blocked: result.blocked,
                    error: result.error.clone(),
                })
                .await;
        }
        hook_results
    }

    async fn run_plain(
        &self,
        runner: &aemeath_core::hook::HookRunner,
        event: HookEvent,
        tool_name: Option<&str>,
        data: HookData,
    ) -> Vec<HookResult> {
        self.run_json(runner, event, tool_name, data)
            .await
            .into_iter()
            .map(|(_, result, _)| result)
            .collect()
    }
}

fn hook_event_name(event: HookEvent) -> &'static str {
    match event {
        HookEvent::PreToolUse => "PreToolUse",
        HookEvent::PostToolUse => "PostToolUse",
        HookEvent::PostToolUseFailure => "PostToolUseFailure",
        HookEvent::UserPromptSubmit => "UserPromptSubmit",
        HookEvent::Stop => "Stop",
        HookEvent::StopFailure => "StopFailure",
        HookEvent::SessionStart => "SessionStart",
        HookEvent::SessionEnd => "SessionEnd",
        HookEvent::PreCompact => "PreCompact",
        HookEvent::PostCompact => "PostCompact",
        HookEvent::PostToolBatch => "PostToolBatch",
        HookEvent::SubagentStart => "SubagentStart",
        HookEvent::SubagentStop => "SubagentStop",
        HookEvent::TaskCreated => "TaskCreated",
        HookEvent::TaskCompleted => "TaskCompleted",
        HookEvent::PermissionRequest => "PermissionRequest",
        HookEvent::PermissionDenied => "PermissionDenied",
        HookEvent::Notification => "Notification",
        HookEvent::InstructionsLoaded => "InstructionsLoaded",
        HookEvent::ConfigChange => "ConfigChange",
        HookEvent::Elicitation => "Elicitation",
        HookEvent::ElicitationResult => "ElicitationResult",
        HookEvent::UserPromptExpansion => "UserPromptExpansion",
        HookEvent::CwdChanged => "CwdChanged",
        HookEvent::FileChanged => "FileChanged",
        HookEvent::TeammateIdle => "TeammateIdle",
    }
}

async fn drain_queued_input(tx: &mpsc::Sender<UiEvent>) -> Option<Vec<String>> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    if tx
        .send(UiEvent::DrainQueuedInput { reply_tx })
        .await
        .is_err()
    {
        return None;
    }
    match reply_rx.await {
        Ok(queued) if !queued.is_empty() => Some(queued),
        _ => None,
    }
}

use super::done::input_queue_preview;
use super::spinner::{short_hook_command, truncate_for_spinner};
use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::processing::SpawnContextRefs;
use crate::tui::app::{App, UiEvent};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn build_option_line_ranges(start: usize, options: &[String]) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::with_capacity(options.len());
    let mut next = start;
    for option in options {
        let line_count = option.lines().count().max(1);
        ranges.push(next..next + line_count);
        next += line_count;
    }
    ranges
}

impl App {
    /// Handle UI events from background processing
    pub(super) fn update_ui(
        &mut self,
        ev: UiEvent,
        ui_tx: &mpsc::Sender<UiEvent>,
        _active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        _spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        match ev {
            UiEvent::Text(text) => {
                if self.tool_call_active {
                    log::debug!("[SPINNER] Text: tool_call_active was true, resetting to false");
                    self.tool_call_active = false;
                    self.active_tool_call_ids.clear();
                }
                self.output_area.set_spinner_phase("Generating...");
                self.output_area.append_assistant_text(&text);
            }
            UiEvent::Thinking(text) => {
                if self.tool_call_active {
                    log::debug!(
                        "[SPINNER] Thinking: tool_call_active was true, resetting to false"
                    );
                    self.tool_call_active = false;
                    self.active_tool_call_ids.clear();
                }
                self.output_area.set_spinner_phase("Thinking...");
                self.output_area.append_thinking_text(&text);
            }
            UiEvent::TextBlockComplete(_text) => {
                self.output_area.finish_streaming();
                self.output_area.push_system("");
            }
            UiEvent::ToolCallStart { name, index } => {
                log::debug!(
                    "[SPINNER] ToolCallStart({name}[{index}]): tool_call_active {} -> true",
                    self.tool_call_active
                );
                self.tool_call_active = true;
                self.output_area.push_tool_call_start(&name, index);
                // AskUserQuestion 等待用户回复期间不应显示 spinner
                if name != "AskUserQuestion" {
                    self.output_area.start_spinner();
                    self.output_area
                        .set_spinner_phase(format!("Calling {name}..."));
                }
            }
            UiEvent::ToolArgumentsDelta {
                index,
                name,
                partial_args,
            } => {
                self.output_area
                    .update_tool_call_pending(&name, index, &partial_args);
            }
            UiEvent::ToolCall { id, name, summary } => {
                log::debug!(
                    "[SPINNER] ToolCall({name}): tool_call_active={}",
                    self.tool_call_active
                );
                self.tool_call_active = true;
                self.active_tool_call_ids.insert(id.clone());
                self.output_area.push_tool_call(&id, &name, &summary);
                self.output_area.start_spinner();
                self.output_area
                    .set_spinner_phase(format!("Calling {name}..."));
            }
            UiEvent::ToolResult {
                id,
                tool_name,
                output,
                is_error,
                images,
            } => {
                let image_note = if images.is_empty() {
                    String::new()
                } else {
                    format!("  │  [{} image(s) attached]\n", images.len())
                };
                self.output_area.push_tool_result_with_diff(
                    &id,
                    &tool_name,
                    &output,
                    is_error,
                    &image_note,
                );
                let had_active_id = self.active_tool_call_ids.remove(&id);
                let remaining = self.active_tool_call_ids.len();
                log::debug!(
                    "[BUG#24] ToolResult({tool_name}): removed_id={had_active_id}, remaining_active_tools={remaining}"
                );
                if remaining == 0 {
                    // All tool results received — agent loop will continue with next API call.
                    // Restart spinner to show "waiting for next response" state.
                    self.tool_call_active = false;
                    self.output_area.start_spinner();
                    self.output_area.set_spinner_phase("Thinking...");
                } else {
                    self.tool_call_active = true;
                    self.output_area.start_spinner();
                    self.output_area
                        .set_spinner_phase(format!("Calling tools... ({remaining} running)"));
                }
            }
            UiEvent::Usage {
                input,
                output,
                last_input,
                elapsed_secs,
            } => {
                self.total_input_tokens += input as u64;
                self.total_output_tokens += output as u64;
                self.total_api_calls += 1;
                self.last_input_tokens = last_input as u64;
                let tps = if elapsed_secs > 0.0 {
                    output as f64 / elapsed_secs
                } else {
                    0.0
                };
                self.status_bar.set_tps(tps);
            }
            UiEvent::LiveTps(tps) => {
                self.status_bar.set_tps(tps);
            }
            UiEvent::AgentProgress { tool_id, event } => {
                self.output_area.push_agent_progress(&tool_id, event);
                self.output_area.start_spinner();
                self.output_area.set_spinner_phase("Agent working...");
            }
            UiEvent::Error(msg) => {
                log::debug!(
                    "[SPINNER] Error: tool_call_active {} -> false",
                    self.tool_call_active
                );
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                self.is_processing = false;
                return UpdateResult {
                    cmd: Cmd::RunHookNotification {
                        message: msg,
                        kind: "error".to_string(),
                    },
                    pending_slash: None,
                };
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                self.is_processing = false;
            }
            UiEvent::MessagesSync(msgs) => {
                self.messages = msgs;
                return UpdateResult {
                    cmd: Cmd::SaveSession(self.messages.clone()),
                    pending_slash: None,
                };
            }
            UiEvent::ClipboardImage(img) => {
                self.pending_images.push(img);
                self.input_area
                    .set_pending_images(self.pending_images.len());
            }
            UiEvent::SystemMessage(msg) => {
                // Hook notification deferred to Cmd; state update stays here
                self.output_area.push_system(&msg);
                return UpdateResult {
                    cmd: Cmd::RunHookNotification {
                        message: msg,
                        kind: "system_message".to_string(),
                    },
                    pending_slash: None,
                };
            }
            UiEvent::ReflectionStarted => {
                self.output_area.start_spinner();
                self.output_area.set_spinner_phase("Reflecting...");
                self.is_processing = true;
            }
            UiEvent::ReflectionUsage { input, output } => {
                self.total_api_calls += 1;
                self.last_input_tokens = input as u64;
                self.total_input_tokens += input as u64;
                self.total_output_tokens += output as u64;
                self.status_bar.set_tokens(
                    self.total_input_tokens,
                    self.total_output_tokens,
                    self.last_input_tokens,
                );
            }
            UiEvent::ReflectionDone { output } => {
                let formatted =
                    ::runtime::api::core::reflection::ReflectionEngine::format_output(&output);
                self.output_area.push_system(&formatted);
                if self.memory_config.reflection.auto_apply_suggestions {
                    self.apply_reflection_output(output);
                } else {
                    let suggestion_count = output.suggested_memories.len();
                    let outdated_count = output.outdated_memories.len();
                    self.pending_reflection = Some(output);
                    if suggestion_count > 0 || outdated_count > 0 {
                        self.output_area.push_system(&format!(
                            "[reflection: {suggestion_count} 条建议记忆、{outdated_count} 条过时标记待应用；运行 /reflect apply]"
                        ));
                    }
                }
                self.output_area.stop_spinner();
                self.is_processing = false;
                self.status_bar.set_success("Ready");
            }
            UiEvent::AskUser {
                id,
                question,
                options,
                allow_free_input,
                multi_select,
                default,
                reply_tx,
            } => {
                self.active_tool_call_ids.remove(&id);
                self.tool_call_active = !self.active_tool_call_ids.is_empty();
                self.output_area.stop_spinner();

                // Append built-in options when LLM provides ≥ 1 option
                let llm_option_count = options.len();
                let mut all_options = options.clone();
                if llm_option_count > 0 {
                    all_options.push(
                        crate::tui::app::BUILTIN_OPTION_ALL.to_string(),
                    );
                    all_options.push(
                        crate::tui::app::BUILTIN_OPTION_CHAT.to_string(),
                    );
                }

                let default_ref = default.as_deref();
                let option_line_start =
                    self.output_area
                        .push_ask_user(&question, &all_options, default_ref, multi_select);

                if let Some(start) = option_line_start {
                    let cursor = default
                        .as_ref()
                        .and_then(|d| all_options.iter().position(|o| o == d))
                        .unwrap_or(0);
                    let total = all_options.len();
                    let option_line_ranges = build_option_line_ranges(start, &all_options);
                    self.ask_user_state = Some(crate::tui::app::AskUserState {
                        reply_tx,
                        options: all_options,
                        llm_option_count,
                        cursor,
                        multi_select,
                        selected: vec![false; total],
                        option_line_ranges,
                        allow_free_input,
                        chat_input_active: false,
                    });
                } else {
                    // 无选项：退回自由输入模式
                    self.ask_user_reply_tx = Some(reply_tx);
                }
                self.output_area.stop_spinner();
            }
            UiEvent::StopFailureHook {
                system_message,
                additional_context,
            } => {
                if let Some(ref msg) = system_message {
                    self.output_area.push_system(msg);
                }
                if let Some(ref ctx) = additional_context {
                    self.output_area
                        .push_system(&format!("[Additional Context] {ctx}"));
                }
                if let Some(msg) = system_message {
                    return UpdateResult {
                        cmd: Cmd::RunHookNotification {
                            message: msg,
                            kind: "system_message".to_string(),
                        },
                        pending_slash: None,
                    };
                }
            }
            UiEvent::DrainQueuedInput { reply_tx } => {
                let queued: Vec<String> = self.input_queue.drain(..).collect();
                if !queued.is_empty() {
                    let flushed: Vec<String> = self.output_area.queued_messages.drain(..).collect();
                    for msg in &flushed {
                        self.output_area.push_user_message(msg);
                    }
                    self.output_area
                        .set_spinner_phase("Thinking with queued input...");
                }
                let _ = reply_tx.send(queued);
            }
            UiEvent::HookStart { event, command } => {
                self.output_area.start_spinner();
                self.output_area
                    .set_spinner_phase(format!("Hook {event}: {}", short_hook_command(&command)));
            }
            UiEvent::HookEnd {
                event,
                blocked,
                error,
            } => {
                if blocked {
                    self.output_area
                        .set_spinner_phase(format!("Hook {event} blocked"));
                } else if let Some(error) = error {
                    self.output_area.set_spinner_phase(format!(
                        "Hook {event} failed: {}",
                        truncate_for_spinner(&error, 48)
                    ));
                } else {
                    self.output_area
                        .set_spinner_phase(format!("Hook {event} done"));
                }
            }
            UiEvent::WorkingDirectoryChanged(ctx) => {
                self.cwd = ctx.raw_path_base.clone();
                self.workspace_context = Some(ctx.workspace.clone());
                self.status_bar
                    .set_context_paths(ctx.path_base, ctx.working_root);
                if let Some(branch) = ctx.branch {
                    self.status_bar.set_git_context(ctx.kind, branch);
                } else {
                    self.status_bar.set_git_context(ctx.kind, "");
                }
            }
            UiEvent::Done => {
                log::debug!(
                    "[SPINNER] Done: tool_call_active {} -> false",
                    self.tool_call_active
                );
                log::info!(
                    "[bug49_input_queue_at_done] session_id={} event=Done input_queue_len={} queued_messages_len={} is_processing={} tool_call_active={} active_tool_call_ids={} input_area_empty={} input_queue_front_preview={:?}",
                    self.session_id,
                    self.input_queue.len(),
                    self.output_area.queued_messages.len(),
                    self.is_processing,
                    self.tool_call_active,
                    self.active_tool_call_ids.len(),
                    self.input_area.is_empty(),
                    input_queue_preview(&self.input_queue)
                );
                self.handle_done(ui_tx, None);
            }
            UiEvent::DoneWithDuration(elapsed) => {
                log::debug!(
                    "[SPINNER] DoneWithDuration: tool_call_active {} -> false",
                    self.tool_call_active
                );
                log::info!(
                    "[bug49_input_queue_at_done] session_id={} event=DoneWithDuration input_queue_len={} queued_messages_len={} is_processing={} tool_call_active={} active_tool_call_ids={} input_area_empty={} input_queue_front_preview={:?}",
                    self.session_id,
                    self.input_queue.len(),
                    self.output_area.queued_messages.len(),
                    self.is_processing,
                    self.tool_call_active,
                    self.active_tool_call_ids.len(),
                    self.input_area.is_empty(),
                    input_queue_preview(&self.input_queue)
                );
                self.handle_done(ui_tx, Some(elapsed));
            }
        }

        UpdateResult {
            cmd: Cmd::None,
            pending_slash: None,
        }
    }
}

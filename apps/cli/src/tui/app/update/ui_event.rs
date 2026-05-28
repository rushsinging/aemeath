use super::ask_user_options::build_option_line_ranges;
use super::spinner::{short_hook_command, truncate_for_spinner};
use super::UpdateResult;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::session::processing::SpawnContextRefs;
use tokio::sync::mpsc;

impl App {
    /// Handle UI events from background processing
    pub(super) fn update_ui(
        &mut self,
        ev: UiEvent,
        ui_tx: &mpsc::Sender<UiEvent>,
        _spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let mut effects = Vec::new();
        match ev {
            UiEvent::Text(_text) => {
                if self.chat.tool_call_active {
                    log::debug!("[SPINNER] Text: tool_call_active was true, resetting to false");
                    self.chat.clear_tool_activity();
                }
                self.output_area.set_spinner_phase("Generating...");
            }
            UiEvent::Thinking(_text) => {
                if self.chat.tool_call_active {
                    log::debug!(
                        "[SPINNER] Thinking: tool_call_active was true, resetting to false"
                    );
                    self.chat.clear_tool_activity();
                }
                self.output_area.set_spinner_phase("Thinking...");
            }
            UiEvent::TextBlockComplete(_text) => {}
            UiEvent::ToolCallStart { name, index } => {
                log::debug!(
                    "[SPINNER] ToolCallStart({name}[{index}]): tool_call_active {} -> true",
                    self.chat.tool_call_active
                );
                self.chat.start_tool_activity();
                // AskUserQuestion 等待用户回复期间不应显示 spinner
                if name != "AskUserQuestion" {
                    self.output_area.start_spinner();
                    self.output_area
                        .set_spinner_phase(format!("Calling {name}..."));
                }
            }
            UiEvent::ToolArgumentsDelta {
                index: _,
                name: _,
                partial_args: _,
            } => {}
            UiEvent::ToolCall {
                id,
                name,
                index: _,
                summary: _,
            } => {
                log::debug!(
                    "[SPINNER] ToolCall({name}): tool_call_active={}",
                    self.chat.tool_call_active
                );
                self.chat.register_tool_call(id.clone());
                self.output_area.start_spinner();
                self.output_area
                    .set_spinner_phase(format!("Calling {name}..."));
            }
            UiEvent::ToolResult {
                id,
                tool_name,
                output: _,
                is_error: _,
                images: _,
            } => {
                let had_active_id = self.chat.has_active_tool_call(&id);
                let remaining = self.chat.finish_tool_call(&id);
                log::debug!(
                    "[BUG#24] ToolResult({tool_name}): removed_id={had_active_id}, remaining_active_tools={remaining}"
                );
                if remaining == 0 {
                    // All tool results received — agent loop will continue with next API call.
                    // Restart spinner to show "waiting for next response" state.
                    self.output_area.start_spinner();
                    self.output_area.set_spinner_phase("Thinking...");
                } else {
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
                self.chat
                    .record_usage(input as u64, output as u64, last_input as u64);
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
                    self.chat.tool_call_active
                );
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                self.chat.stop_processing();
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "error".to_string(),
                });
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                self.chat.stop_processing();
            }
            UiEvent::MessagesSync(msgs) => {
                self.chat.messages = msgs;
                return UpdateResult::one(Effect::SaveSession);
            }
            UiEvent::ClipboardImage(img) => {
                let count = self.chat.add_pending_image(img);
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::SetAttachmentCount(count),
                );
            }
            UiEvent::SystemMessage(msg) => {
                // Hook notification deferred to Cmd; state update stays here
                self.output_area.push_system(&msg);
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "system_message".to_string(),
                });
            }
            UiEvent::ReminderRecap(line) => {
                self.handle_reminder_recap(&line);
            }
            UiEvent::MemoryList(reminders) => {
                self.handle_memory_list(&reminders);
            }
            UiEvent::ReflectionStarted => {
                self.output_area.start_spinner();
                self.output_area.set_spinner_phase("Reflecting...");
                self.chat.start_processing();
            }
            UiEvent::ReflectionUsage { input, output } => {
                self.chat
                    .record_usage(input as u64, output as u64, input as u64);
                self.status_bar.set_tokens(
                    self.chat.total_input_tokens,
                    self.chat.total_output_tokens,
                    self.chat.last_input_tokens,
                );
            }
            UiEvent::ReflectionDone { output } => {
                self.output_area.push_system(&output.content);
                if self.session.memory_config.reflection.auto_apply_suggestions {
                    self.apply_reflection_output(output);
                } else {
                    let suggestion_count = output.suggested_memories.len();
                    let outdated_count = output.outdated_memories.len();
                    self.chat.pending_reflection = Some(output);
                    if suggestion_count > 0 || outdated_count > 0 {
                        self.output_area.push_system(&format!(
                            "[reflection: {suggestion_count} 条建议记忆、{outdated_count} 条过时标记待应用；运行 /reflect apply]"
                        ));
                    }
                }
                self.output_area.stop_spinner();
                self.chat.stop_processing();
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
                self.chat.finish_tool_call(&id);
                self.output_area.stop_spinner();

                // Append built-in options when LLM provides ≥ 1 option
                let llm_option_count = options.len();
                let mut all_options = options.clone();
                if llm_option_count > 0 {
                    all_options.push(crate::tui::app::state::BUILTIN_OPTION_ALL.to_string());
                    all_options.push(crate::tui::app::state::BUILTIN_OPTION_CHAT.to_string());
                }

                let default_ref = default.as_deref();
                let option_line_start = self.output_area.push_ask_user(
                    &question,
                    &all_options,
                    default_ref,
                    multi_select,
                );

                if let Some(start) = option_line_start {
                    let cursor = default
                        .as_ref()
                        .and_then(|d| all_options.iter().position(|o| o == d))
                        .unwrap_or(0);
                    let total = all_options.len();
                    let option_line_ranges = build_option_line_ranges(start, &all_options);
                    self.input.ask_user_state = Some(crate::tui::app::state::AskUserState {
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
                    self.input.ask_user_reply_tx = Some(reply_tx);
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
                    return UpdateResult::one(Effect::RunHook {
                        message: msg,
                        name: "system_message".to_string(),
                    });
                }
            }
            UiEvent::DrainQueuedInput { reply_tx } => {
                let queued = self.input.drain_queue();
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
            UiEvent::CurrentTurnChanged(turn) => {
                return UpdateResult::one(Effect::SetCurrentTurn { turn });
            }
            UiEvent::WorkingDirectoryChanged(ctx) => {
                self.session.cwd = ctx.raw_path_base.clone();
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
                    self.chat.tool_call_active
                );
                log::info!(
                    "[bug49_input_queue_at_done] session_id={} event=Done input_queue_len={} queued_messages_len={} is_processing={} tool_call_active={} active_tool_call_ids={} input_area_empty={} input_queue_front_preview={:?}",
                    self.session.session_id,
                    self.input.queue_len(),
                    self.output_area.queued_messages.len(),
                    self.chat.is_processing,
                    self.chat.tool_call_active,
                    self.chat.active_tool_call_ids.len(),
                    self.model.input.document.is_empty(),                    self.input.queue_preview()
                );
                if let Some(effect) = self.handle_done(ui_tx, None) {
                    effects.push(effect);
                }
            }
            UiEvent::DoneWithDuration(elapsed) => {
                log::debug!(
                    "[SPINNER] DoneWithDuration: tool_call_active {} -> false",
                    self.chat.tool_call_active
                );
                log::info!(
                    "[bug49_input_queue_at_done] session_id={} event=DoneWithDuration input_queue_len={} queued_messages_len={} is_processing={} tool_call_active={} active_tool_call_ids={} input_area_empty={} input_queue_front_preview={:?}",
                    self.session.session_id,
                    self.input.queue_len(),
                    self.output_area.queued_messages.len(),
                    self.chat.is_processing,
                    self.chat.tool_call_active,
                    self.chat.active_tool_call_ids.len(),
                    self.model.input.document.is_empty(),                    self.input.queue_preview()
                );
                if let Some(effect) = self.handle_done(ui_tx, Some(elapsed)) {
                    effects.push(effect);
                }
            }
        }

        UpdateResult {
            effects,
            spawn_effect: None,
            pending_slash: None,
        }
    }
}

use super::spinner::{short_hook_command, truncate_for_spinner};
use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::processing::SpawnContextRefs;
use crate::tui::app::{App, UiEvent};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    /// Handle UI events from background processing
    pub(super) fn update_ui(
        &mut self,
        ev: UiEvent,
        _ui_tx: &mpsc::Sender<UiEvent>,
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
            UiEvent::ToolCallStart(name) => {
                log::debug!(
                    "[SPINNER] ToolCallStart({name}): tool_call_active {} -> true",
                    self.tool_call_active
                );
                self.tool_call_active = true;
                self.output_area.push_tool_call_start(&name);
                // AskUserQuestion 等待用户回复期间不应显示 spinner
                if name != "AskUserQuestion" {
                    self.output_area.start_spinner();
                    self.output_area
                        .set_spinner_phase(format!("Calling {name}..."));
                }
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
                let default_ref = default.as_deref();
                let option_line_start =
                    self.output_area
                        .push_ask_user(&question, &options, default_ref, multi_select);

                if let Some(start) = option_line_start {
                    let cursor = default
                        .as_ref()
                        .and_then(|d| options.iter().position(|o| o == d))
                        .unwrap_or(0);
                    self.ask_user_state = Some(crate::tui::app::AskUserState {
                        reply_tx,
                        options: options.clone(),
                        cursor,
                        multi_select,
                        selected: vec![false; options.len()],
                        option_line_start: start,
                        allow_free_input,
                    });
                } else {
                    // 无选项：退回自由输入模式
                    self.ask_user_reply_tx = Some(reply_tx);
                }
                self.output_area.set_spinner_phase("Waiting for user");
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
            UiEvent::Done => {
                log::debug!(
                    "[SPINNER] Done: tool_call_active {} -> false",
                    self.tool_call_active
                );
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                self.is_processing = false;
                self.status_bar.set_success("Ready");
                if let Ok(reminders) = self.session_reminders.lock() {
                    if let Some(line) = reminders.recap_line() {
                        self.output_area.push_system(&line);
                    }
                }
            }
            UiEvent::DoneWithDuration(elapsed) => {
                log::debug!(
                    "[SPINNER] DoneWithDuration: tool_call_active {} -> false",
                    self.tool_call_active
                );
                self.output_area.push_done(elapsed);
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                self.is_processing = false;
                self.status_bar.set_success("Ready");
                if let Ok(reminders) = self.session_reminders.lock() {
                    if let Some(line) = reminders.recap_line() {
                        self.output_area.push_system(&line);
                    }
                }
            }
        }

        UpdateResult {
            cmd: Cmd::None,
            pending_slash: None,
        }
    }
}

use super::UiEvent;
use aemeath_core::message::Message;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl super::App {
    /// [DEPRECATED] Logic moved to update.rs. Kept for reference only.
    #[allow(dead_code)]
    /// Handle a UiEvent from the background task.
    /// Returns the updated `is_processing` flag.
    pub(super) async fn handle_ui_event(
        &mut self,
        ev: UiEvent,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_ctx: &super::processing::SpawnContextRefs<'_>,
    ) {
        match ev {
            UiEvent::Text(text) => {
                if self.tool_call_active {
                    log::debug!("[SPINNER] Text: tool_call_active was true, resetting to false");
                    self.tool_call_active = false;
                }
                self.output_area.set_spinner_phase("Generating...");
                self.output_area.stop_spinner();
                self.output_area.append_assistant_text(&text);
            }
            UiEvent::Thinking(text) => {
                if self.tool_call_active {
                    log::debug!(
                        "[SPINNER] Thinking: tool_call_active was true, resetting to false"
                    );
                    self.tool_call_active = false;
                }
                self.output_area.set_spinner_phase("Thinking...");
                self.output_area.stop_spinner();
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
                }
            }
            UiEvent::ToolCall { id, name, summary } => {
                log::debug!(
                    "[SPINNER] ToolCall({name}): tool_call_active={}",
                    self.tool_call_active
                );
                self.output_area.push_tool_call(&id, &name, &summary);
                self.output_area.start_spinner();
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
                log::debug!("[BUG#24] ToolResult({tool_name}): restarting spinner for next turn");
                self.tool_call_active = false;
                self.output_area.set_spinner_phase("Thinking...");
                self.output_area.start_spinner();
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
            UiEvent::Error(msg) => {
                log::debug!(
                    "[SPINNER] Error: tool_call_active {} -> false",
                    self.tool_call_active
                );
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
            }
            UiEvent::MessagesSync(msgs) => {
                self.messages = msgs;
                if !self.messages.is_empty() {
                    use aemeath_core::session::{self as sess, Session};
                    let mut s = Session::new(
                        self.session_id.clone(),
                        self.cwd.to_string_lossy().to_string(),
                    );
                    s.messages = self.messages.clone();
                    s.created_at = self
                        .session_created_at
                        .clone()
                        .unwrap_or_else(|| aemeath_core::session::now_iso());
                    s.updated_at = aemeath_core::session::now_iso();
                    if let Err(e) = sess::save_session(&s).await {
                        log::warn!("failed to auto-save session on sync: {e}");
                    }
                }
            }
            UiEvent::ClipboardImage(img) => {
                self.pending_images.push(img);
                self.input_area
                    .set_pending_images(self.pending_images.len());
            }
            UiEvent::SystemMessage(msg) => {
                self.output_area.push_system(&msg);
            }
            UiEvent::AskUser {
                id,
                question,
                options,
                allow_free_input: _,
                multi_select: _,
                default,
                reply_tx,
            } => {
                // DEPRECATED path — logic moved to update.rs; keep for compilation
                let _ = (id, question, options, default, reply_tx);
            }
            UiEvent::Done => {
                log::debug!(
                    "[SPINNER] Done: tool_call_active {} -> false",
                    self.tool_call_active
                );
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
                self.status_bar.set_success("Ready");
                if !self.input_queue.is_empty() {
                    let flushed: Vec<String> = self.output_area.queued_messages.drain(..).collect();
                    for msg in &flushed {
                        self.output_area.push_user_message(msg);
                    }
                    let queued: Vec<String> = self.input_queue.drain(..).collect();
                    self.start_queued_processing_batch(
                        queued,
                        is_processing,
                        ui_tx,
                        active_cancel,
                        spawn_ctx,
                    );
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
                *is_processing = false;
                self.status_bar.set_success("Ready");
                if !self.input_queue.is_empty() {
                    let flushed: Vec<String> = self.output_area.queued_messages.drain(..).collect();
                    for msg in &flushed {
                        self.output_area.push_user_message(msg);
                    }
                    let queued: Vec<String> = self.input_queue.drain(..).collect();
                    self.start_queued_processing_batch(
                        queued,
                        is_processing,
                        ui_tx,
                        active_cancel,
                        spawn_ctx,
                    );
                }
            }
            UiEvent::AgentProgress { .. } => {
                // Sub-agent progress is no longer displayed on the header line
            }
            UiEvent::StopFailureHook { .. } => {
                // Handled in update.rs (update_ui)
            }
            UiEvent::DrainQueuedInput { reply_tx } => {
                let queued: Vec<String> = self.input_queue.drain(..).collect();
                let flushed: Vec<String> = self.output_area.queued_messages.drain(..).collect();
                for msg in &flushed {
                    self.output_area.push_user_message(msg);
                }
                let _ = reply_tx.send(queued);
            }
            UiEvent::HookStart { .. } | UiEvent::HookEnd { .. } => {
                // Hook lifecycle events — handled in update.rs for spinner display
            }
        }
    }

    /// Start processing all queued input messages as a single batch.
    #[allow(dead_code)]
    fn start_queued_processing_batch(
        &mut self,
        queued: Vec<String>,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_ctx: &super::processing::SpawnContextRefs<'_>,
    ) {
        spawn_ctx.interrupted.store(false, Ordering::Relaxed);
        for msg in &queued {
            self.messages.push(Message::user(msg));
        }
        self.output_area.set_spinner_phase("Thinking...");
        self.output_area.start_spinner();
        *is_processing = true;

        let cancel = CancellationToken::new();
        if let Ok(mut guard) = active_cancel.lock() {
            *guard = Some(cancel.clone());
        }

        super::processing::spawn_processing(super::processing::SpawnContext {
            tx: ui_tx.clone(),
            queue_request_tx: ui_tx.clone(),
            client: spawn_ctx.client.clone(),
            registry: spawn_ctx.registry.clone(),
            system_blocks: spawn_ctx.system_blocks.clone(),
            system_prompt_text: spawn_ctx.system_prompt_text.to_string(),
            user_context: spawn_ctx.user_context.to_string(),
            messages: self.messages.clone(),
            context_size: spawn_ctx.context_size,
            cwd: self.cwd.clone(),
            session_id: self.session_id.clone(),
            read_files: spawn_ctx.read_files.clone(),
            session_reminders: spawn_ctx.session_reminders.clone(),
            agent_runner: spawn_ctx.agent_runner.clone(),
            allow_all: spawn_ctx.allow_all,
            interrupted: spawn_ctx.interrupted.clone(),
            cancel,
            task_store: spawn_ctx.task_store.clone(),
            max_tool_concurrency: spawn_ctx.max_tool_concurrency,
            max_agent_concurrency: spawn_ctx.max_agent_concurrency,
            agent_semaphore: spawn_ctx.agent_semaphore.clone(),
            hook_runner: spawn_ctx.hook_runner.clone(),
            memory_config: spawn_ctx.memory_config.clone(),
        });
    }
}

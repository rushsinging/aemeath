use super::UiEvent;
use aemeath_core::message::Message;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl super::App {
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
                self.output_area.stop_spinner();
                self.output_area.append_assistant_text(&text);
            }
            UiEvent::Thinking(text) => {
                self.output_area.stop_spinner();
                self.output_area.append_thinking_text(&text);
            }
            UiEvent::TextBlockComplete(_text) => {
                self.output_area.finish_streaming();
                self.output_area.push_system("");
            }
            UiEvent::ToolCallStart(name) => {
                self.output_area.start_spinner();
                self.status_bar.set_processing(&format!("Calling {}...", name));
            }
            UiEvent::ToolCall { id, name, summary } => {
                self.output_area.push_tool_call(&id, &name, &summary);
                self.output_area.start_spinner();
            }
            UiEvent::ToolResult { id, tool_name, output, is_error, images } => {
                let image_note = if images.is_empty() {
                    String::new()
                } else {
                    format!("  │  [{} image(s) attached]\n", images.len())
                };
                self.output_area.push_tool_result_with_diff(&id, &tool_name, &output, is_error, &image_note);
            }
            UiEvent::Usage { input, output, last_input, elapsed_secs } => {
                self.total_input_tokens += input as u64;
                self.total_output_tokens += output as u64;
                self.total_api_calls += 1;
                self.last_input_tokens = last_input as u64;
                let tps = if elapsed_secs > 0.0 { output as f64 / elapsed_secs } else { 0.0 };
                self.status_bar.set_tps(tps);
                }
                UiEvent::Error(msg) => {
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                *is_processing = false;
                self.status_bar.clear_processing();
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                *is_processing = false;
                self.status_bar.clear_processing();
            }
            UiEvent::MessagesSync(msgs) => {
                self.messages = msgs;
                if !self.messages.is_empty() {
                    use aemeath_core::session::{self as sess, Session, now_iso};
                    let s = Session {
                        id: self.session_id.clone(),
                        cwd: self.cwd.to_string_lossy().to_string(),
                        messages: self.messages.clone(),
                        created_at: self.session_created_at.clone().unwrap_or_else(now_iso),
                        updated_at: now_iso(),
                        metadata: Default::default(),
                    };
                    if let Err(e) = sess::save_session(&s).await {
                        log::warn!("failed to auto-save session on sync: {e}");
                    }
                }
            }
            UiEvent::ClipboardImage(img) => {
                self.pending_images.push(img);
                self.input_area.set_pending_images(self.pending_images.len());
            }
            UiEvent::SystemMessage(msg) => {
                self.output_area.push_system(&msg);
            }
            UiEvent::Done => {
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                *is_processing = false;
                self.status_bar.clear_processing();
                self.status_bar.set_success("Ready");
                if let Some(queued) = self.queued_input.take() {
                    self.start_queued_processing(queued, is_processing, ui_tx, active_cancel, spawn_ctx);
                }
            }
            UiEvent::DoneWithDuration(elapsed) => {
                self.output_area.push_done(elapsed);
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                *is_processing = false;
                self.status_bar.clear_processing();
                self.status_bar.set_success("Ready");
                if let Some(queued) = self.queued_input.take() {
                    self.start_queued_processing(queued, is_processing, ui_tx, active_cancel, spawn_ctx);
                }
            }
        }
    }

    /// Start processing a queued input message.
    fn start_queued_processing(
        &mut self,
        queued: String,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_ctx: &super::processing::SpawnContextRefs<'_>,
    ) {
        spawn_ctx.interrupted.store(false, Ordering::Relaxed);
        self.messages.push(Message::user(&queued));
        self.status_bar.set_processing("Thinking...");
        self.output_area.start_spinner();
        *is_processing = true;

        let cancel = CancellationToken::new();
        if let Ok(mut guard) = active_cancel.lock() {
            *guard = Some(cancel.clone());
        }

        super::processing::spawn_processing(super::processing::SpawnContext {
            tx: ui_tx.clone(),
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
            agent_runner: spawn_ctx.agent_runner.clone(),
            allow_all: spawn_ctx.allow_all,
            interrupted: spawn_ctx.interrupted.clone(),
            cancel,
            task_store: spawn_ctx.task_store.clone(),
            max_tool_concurrency: spawn_ctx.max_tool_concurrency,
            max_agent_concurrency: spawn_ctx.max_agent_concurrency,
            agent_semaphore: spawn_ctx.agent_semaphore.clone(),
        });
    }
}

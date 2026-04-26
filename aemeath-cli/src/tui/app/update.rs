use super::msg::{Cmd, Msg};
use super::processing::{SpawnContext, SpawnContextRefs};
use super::UiEvent;
use aemeath_core::message::Message;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Return type for update: (commands, whether to continue the loop)
pub struct UpdateResult {
    pub cmd: Cmd,
    pub pending_slash: Option<String>,
}

impl App {
    /// TEA-style update: pure state transition based on a message.
    /// Returns commands for the runtime to execute.
    pub(crate) fn update(
        &mut self,
        msg: Msg,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        match msg {
            Msg::Key(key) => self.update_key(key, is_processing, ui_tx, active_cancel, spawn_refs),
            Msg::Mouse(mouse) => {
                self.handle_mouse_event(mouse, self.output_area_rect);
                UpdateResult { cmd: Cmd::None, pending_slash: None }
            }
            Msg::Paste(text) if !*is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult { cmd: Cmd::None, pending_slash: None }
            }
            Msg::Paste(_) => UpdateResult { cmd: Cmd::None, pending_slash: None },
            Msg::Resize(_, _) => UpdateResult { cmd: Cmd::None, pending_slash: None },
            Msg::Tick => UpdateResult { cmd: Cmd::None, pending_slash: None },
            Msg::Ui(ev) => self.update_ui(ev, is_processing, ui_tx, active_cancel, spawn_refs),
        }
    }

    fn update_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        if key.kind != KeyEventKind::Press {
            return UpdateResult { cmd: Cmd::None, pending_slash: None };
        }

        // Dialog mode
        if self.active_dialog.is_some() {
            match key.code {
                KeyCode::Up => { if let Some(ref mut d) = self.active_dialog { d.select_prev(); } }
                KeyCode::Down => { if let Some(ref mut d) = self.active_dialog { d.select_next(); } }
                KeyCode::Enter => {
                    let selected = self.active_dialog.as_ref().and_then(|d| d.get_selected());
                    if let Some(idx) = selected {
                        if idx < self.dialog_model_keys.len() {
                            let model_key = self.dialog_model_keys[idx].clone();
                            self.queued_input = Some(format!("/model {}", model_key));
                            self.active_dialog = None;
                            self.dialog_model_keys.clear();
                            return UpdateResult { cmd: Cmd::None, pending_slash: Some(format!("/model {}", model_key)) };
                        }
                    }
                    self.active_dialog = None;
                    self.dialog_model_keys.clear();
                }
                KeyCode::Esc => {
                    self.active_dialog = None;
                    self.dialog_model_keys.clear();
                }
                _ => {}
            }
            return UpdateResult { cmd: Cmd::None, pending_slash: None };
        }

        // Shift+Enter / Alt+Enter = insert newline
        if (key.code == KeyCode::Enter || key.code == KeyCode::Char('\n'))
            && key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.input_area.enter(true);
            return UpdateResult { cmd: Cmd::None, pending_slash: None };
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if *is_processing {
                    spawn_refs.interrupted.store(true, Ordering::Relaxed);
                    if let Ok(guard) = active_cancel.lock() {
                        if let Some(token) = guard.as_ref() { token.cancel(); }
                    }
                    self.status_bar.set_warning("Interrupted");
                } else if self.input_area.is_showing_suggestions() {
                    self.input_area.clear_suggestions();
                } else {
                    let now = std::time::Instant::now();
                    if let Some(last) = self.last_ctrlc {
                        if now.duration_since(last).as_secs_f64() < 3.0 {
                            return UpdateResult { cmd: Cmd::Quit, pending_slash: None };
                        } else {
                            self.last_ctrlc = Some(now);
                            self.status_bar.set_warning("Press Ctrl+C again to exit");
                        }
                    } else {
                        self.last_ctrlc = Some(now);
                        self.status_bar.set_warning("Press Ctrl+C again to exit");
                    }
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab) if !*is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Esc) if !*is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.input_area.clear_suggestions();
                }
            }
            (_, KeyCode::Enter) if *is_processing => {
                if !self.input_area.is_empty() {
                    let input = self.input_area.get_text();
                    self.output_area.push_user_message(&input);
                    self.input_area.add_history(&input);
                    self.input_area.clear();
                    self.queued_input = Some(input);
                    self.status_bar.set_warning("Message queued");
                }
            }
            (_, KeyCode::Enter) if !*is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else if !self.input_area.is_empty() {
                    return self.update_enter(is_processing, ui_tx, active_cancel, spawn_refs);
                }
            }
            (KeyModifiers::NONE, KeyCode::PageUp) => self.output_area.scroll_up(10),
            (KeyModifiers::NONE, KeyCode::PageDown) => self.output_area.scroll_down(10),
            (KeyModifiers::SHIFT, KeyCode::Up) => self.output_area.scroll_up(1),
            (KeyModifiers::SHIFT, KeyCode::Down) => self.output_area.scroll_down(1),
            (KeyModifiers::SHIFT, KeyCode::Home) => self.output_area.scroll_up(self.output_area.line_count()),
            (KeyModifiers::SHIFT, KeyCode::End) => self.output_area.scroll_to_bottom(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else { c };
                self.input_area.input(ch);
                if !*is_processing { self.update_suggestions(); }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input_area.backspace();
                if !*is_processing { self.update_suggestions(); }
            }
            (KeyModifiers::NONE, KeyCode::Left) => { self.input_area.move_left(); self.input_area.clear_suggestions(); }
            (KeyModifiers::NONE, KeyCode::Right) => { self.input_area.move_right(); self.input_area.clear_suggestions(); }
            (KeyModifiers::NONE, KeyCode::Up) => self.input_area.move_up(),
            (KeyModifiers::NONE, KeyCode::Down) => self.input_area.move_down(),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.input_area.move_home(),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.input_area.move_end(),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.input_area.delete_word(),
            (KeyModifiers::CONTROL | KeyModifiers::SUPER, KeyCode::Char('v')) if !*is_processing && !self.just_pasted => {
                self.just_pasted = true;
                let tx = ui_tx.clone();
                tokio::spawn(async move {
                    tx.send(UiEvent::SystemMessage("[reading clipboard image...]".to_string())).await.ok();
                    match crate::image::read_clipboard_image().await {
                        Ok(img) => {
                            let size = img.final_size;
                            tx.send(UiEvent::ClipboardImage(img)).await.ok();
                            tx.send(UiEvent::SystemMessage(format!(
                                "[clipboard image added ({} bytes). Type message to send.]", size
                            ))).await.ok();
                        }
                        Err(e) => {
                            tx.send(UiEvent::SystemMessage(format!("No image in clipboard: {e}"))).await.ok();
                        }
                    }
                });
            }
            (KeyModifiers::NONE, KeyCode::End) => self.input_area.move_end(),
            _ => {}
        }

        UpdateResult { cmd: Cmd::None, pending_slash: None }
    }

    /// Handle Enter when not processing
    fn update_enter(
        &mut self,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        let input = self.input_area.get_text();
        if input.starts_with('/') {
            self.input_area.add_history(&input);
            self.input_area.clear();
            self.queued_input = Some(input.clone());
            return UpdateResult { cmd: Cmd::None, pending_slash: Some(input) };
        }

        self.output_area.push_user_message(&input);
        self.input_area.add_history(&input);
        self.input_area.clear();

        let images: Vec<(String, String)> = self.pending_images
            .drain(..)
            .map(|img| (img.base64, img.media_type))
            .collect();
        if images.is_empty() {
            self.messages.push(Message::user(&input));
        } else {
            self.messages.push(Message::user_with_images(&input, images));
        }

        let spawn_ctx = self.build_spawn_context(ui_tx, active_cancel, spawn_refs);
        spawn_refs.interrupted.store(false, Ordering::Relaxed);
        self.status_bar.set_processing("Thinking...");
        self.output_area.start_spinner();
        *is_processing = true;

        UpdateResult { cmd: Cmd::SpawnProcessing(spawn_ctx), pending_slash: None }
    }

    /// Handle UI events from background processing
    fn update_ui(
        &mut self,
        ev: UiEvent,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        match ev {
            UiEvent::Text(text) => {
                if self.tool_call_active {
                    log::debug!("[BUG#4] Text: tool_call_active was true, resetting to false");
                    self.tool_call_active = false;
                }
                self.status_bar.set_processing("Generating...");
                self.output_area.stop_spinner();
                self.output_area.append_assistant_text(&text);
            }
            UiEvent::Thinking(text) => {
                if self.tool_call_active {
                    log::debug!("[BUG#4] Thinking: tool_call_active was true, resetting to false");
                    self.tool_call_active = false;
                }
                self.status_bar.set_processing("Thinking...");
                self.output_area.stop_spinner();
                self.output_area.append_thinking_text(&text);
            }
            UiEvent::TextBlockComplete(_text) => {
                self.output_area.finish_streaming();
                self.output_area.push_system("");
            }
            UiEvent::ToolCallStart(name) => {
                log::debug!("[BUG#4] ToolCallStart({name}): tool_call_active {} -> true", self.tool_call_active);
                self.tool_call_active = true;
                self.output_area.push_tool_call_start(&name);
                self.status_bar.set_processing(&format!("Calling {}...", name));
            }
            UiEvent::ToolCall { id, name, summary } => {
                log::debug!("[BUG#4] ToolCall({name}): tool_call_active={}", self.tool_call_active);
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
                log::debug!("[BUG#4] ToolResult({tool_name}): tool_call_active {} -> false", self.tool_call_active);
                self.tool_call_active = false;
                self.status_bar.set_processing("Generating...");
            }
            UiEvent::Usage { input, output, last_input, elapsed_secs } => {
                self.total_input_tokens += input as u64;
                self.total_output_tokens += output as u64;
                self.total_api_calls += 1;
                self.last_input_tokens = last_input as u64;
                let tps = if elapsed_secs > 0.0 { output as f64 / elapsed_secs } else { 0.0 };
                self.status_bar.set_tps(tps);
            }
            UiEvent::LiveTps(tps) => {
                self.status_bar.set_tps(tps);
            }
            UiEvent::Error(msg) => {
                log::debug!("[BUG#4] Error: tool_call_active {} -> false", self.tool_call_active);
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
                self.status_bar.clear_processing();
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
                self.status_bar.clear_processing();
            }
            UiEvent::MessagesSync(msgs) => {
                self.messages = msgs;
                return UpdateResult { cmd: Cmd::SaveSession(self.messages.clone()), pending_slash: None };
            }
            UiEvent::ClipboardImage(img) => {
                self.pending_images.push(img);
                self.input_area.set_pending_images(self.pending_images.len());
            }
            UiEvent::SystemMessage(msg) => {
                self.output_area.push_system(&msg);
            }
            UiEvent::Done => {
                log::debug!("[BUG#4] Done: tool_call_active {} -> false", self.tool_call_active);
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
                self.status_bar.clear_processing();
                self.status_bar.set_success("Ready");
                if let Some(queued) = self.queued_input.take() {
                    return self.start_queued(queued, is_processing, ui_tx, active_cancel, spawn_refs);
                }
            }
            UiEvent::DoneWithDuration(elapsed) => {
                log::debug!("[BUG#4] DoneWithDuration: tool_call_active {} -> false", self.tool_call_active);
                self.output_area.push_done(elapsed);
                self.output_area.finish_streaming();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                *is_processing = false;
                self.status_bar.clear_processing();
                self.status_bar.set_success("Ready");
                if let Some(queued) = self.queued_input.take() {
                    return self.start_queued(queued, is_processing, ui_tx, active_cancel, spawn_refs);
                }
            }
        }

        UpdateResult { cmd: Cmd::None, pending_slash: None }
    }

    /// Start processing a queued input message
    fn start_queued(
        &mut self,
        queued: String,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        spawn_refs.interrupted.store(false, Ordering::Relaxed);
        self.messages.push(Message::user(&queued));
        self.status_bar.set_processing("Thinking...");
        self.output_area.start_spinner();
        *is_processing = true;

        let spawn_ctx = self.build_spawn_context(ui_tx, active_cancel, spawn_refs);
        UpdateResult { cmd: Cmd::SpawnProcessing(spawn_ctx), pending_slash: None }
    }

    /// Build an owned SpawnContext from borrowed refs
    fn build_spawn_context(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        _active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> SpawnContext {
        let cancel = CancellationToken::new();
        // Note: active_cancel is set by the caller after getting the Cmd back
        SpawnContext {
            tx: ui_tx.clone(),
            client: spawn_refs.client.clone(),
            registry: spawn_refs.registry.clone(),
            system_blocks: spawn_refs.system_blocks.clone(),
            system_prompt_text: spawn_refs.system_prompt_text.to_string(),
            user_context: spawn_refs.user_context.to_string(),
            messages: self.messages.clone(),
            context_size: spawn_refs.context_size,
            cwd: self.cwd.clone(),
            session_id: self.session_id.clone(),
            read_files: spawn_refs.read_files.clone(),
            agent_runner: spawn_refs.agent_runner.clone(),
            allow_all: spawn_refs.allow_all,
            interrupted: spawn_refs.interrupted.clone(),
            cancel,
            task_store: spawn_refs.task_store.clone(),
            max_tool_concurrency: spawn_refs.max_tool_concurrency,
            max_agent_concurrency: spawn_refs.max_agent_concurrency,
            agent_semaphore: spawn_refs.agent_semaphore.clone(),
            hook_runner: spawn_refs.hook_runner.clone(),
        }
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;

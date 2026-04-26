use super::UiEvent;
use aemeath_core::message::Message;
use crossterm::event::{KeyCode, KeyModifiers, KeyEventKind};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Result of handling a key event.
#[derive(Default)]
#[allow(dead_code)]
pub(super) enum KeyResult {
    #[default]
    None,
    /// A slash command input needs async processing (stored in queued_input).
    SlashCommand,
    /// A dialog model switch needs async processing (stored in queued_input).
    DialogModelSwitch,
}

impl super::App {
    /// [DEPRECATED] Logic moved to update.rs. Kept for reference only.
    #[allow(dead_code)]
    pub(super) fn handle_key_event(
        &mut self,
        key: crossterm::event::KeyEvent,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_ctx: &super::processing::SpawnContextRefs<'_>,
    ) -> KeyResult {
        if key.kind != KeyEventKind::Press {
            return KeyResult::None;
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
                            return KeyResult::DialogModelSwitch;
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
            return KeyResult::None;
        }

        // Shift+Enter / Alt+Enter = insert newline
        if (key.code == KeyCode::Enter || key.code == KeyCode::Char('\n'))
            && key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.input_area.enter(true);
            return KeyResult::None;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if *is_processing {
                    spawn_ctx.interrupted.store(true, Ordering::Relaxed);
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
                            self.should_exit = true;
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
                    return self.handle_enter_not_processing(
                        is_processing, ui_tx, active_cancel, spawn_ctx,
                    );
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

        KeyResult::None
    }

    /// Handle Enter when not processing: send message or defer slash command to caller.
    #[allow(dead_code)]
    fn handle_enter_not_processing(
        &mut self,
        is_processing: &mut bool,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_ctx: &super::processing::SpawnContextRefs<'_>,
    ) -> KeyResult {
        let input = self.input_area.get_text();
        if input.starts_with('/') {
            // Slash commands need async — store for caller and add to history
            self.input_area.add_history(&input);
            self.input_area.clear();
            self.queued_input = Some(input);
            return KeyResult::SlashCommand;
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

        spawn_ctx.interrupted.store(false, Ordering::Relaxed);
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
            hook_runner: spawn_ctx.hook_runner.clone(),
        });

        KeyResult::None
    }
}

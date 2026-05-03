use super::msg::{Cmd, Msg};
use super::processing::{SpawnContext, SpawnContextRefs};
use super::UiEvent;
use aemeath_core::message::Message;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn truncate_for_spinner(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn short_hook_command(command: &str) -> String {
    let trimmed = command.trim().trim_matches('"');
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    truncate_for_spinner(tail, 48)
}

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
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Paste(text) if !*is_processing => {
                self.handle_paste_event(text, ui_tx);
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Paste(text) => {
                // Paste while in AskUserQuestion free-input mode: insert into input area only
                if self.ask_user_state.is_some() || self.ask_user_reply_tx.is_some() {
                    self.just_pasted = true;
                    for ch in text.chars() {
                        if ch == '\n' || ch == '\r' {
                            self.input_area.enter(true);
                        } else {
                            self.input_area.input(ch);
                        }
                    }
                    return UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    };
                }
                // Paste while processing: insert into input area so it can be queued
                if text.trim().is_empty() {
                    // Empty paste — try clipboard image (same as idle mode)
                    self.just_pasted = true;
                    let output_tx = ui_tx.clone();
                    tokio::spawn(async move {
                        match crate::image::read_clipboard_image().await {
                            Ok(img) => {
                                let size = img.final_size;
                                let _ = output_tx.send(UiEvent::ClipboardImage(img)).await;
                                let _ = output_tx
                                    .send(UiEvent::SystemMessage(format!(
                                        "[clipboard image added ({} bytes). Type message to send.]",
                                        size
                                    )))
                                    .await;
                            }
                            Err(e) => {
                                let _ = output_tx
                                    .send(UiEvent::Error(format!("No image in clipboard: {e}")))
                                    .await;
                            }
                        }
                    });
                    self.output_area.push_system("[reading clipboard image...]");
                } else if crate::image::is_image_file(text.trim()) {
                    self.output_area
                        .push_system(&format!("[loading image: {}...]", text.trim()));
                    let path = text.trim().to_string();
                    let tx = ui_tx.clone();
                    tokio::spawn(async move {
                        match crate::image::process_image_file(&path).await {
                            Ok(img) => {
                                let size = img.final_size;
                                let _ = tx.send(UiEvent::ClipboardImage(img)).await;
                                let _ = tx
                                    .send(UiEvent::SystemMessage(format!(
                                        "[image loaded ({} bytes). Type message to send.]",
                                        size
                                    )))
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(UiEvent::Error(format!("Failed to load image: {e}")))
                                    .await;
                            }
                        }
                    });
                    self.just_pasted = true;
                } else {
                    self.just_pasted = true;
                    for ch in text.chars() {
                        if ch == '\n' || ch == '\r' {
                            self.input_area.enter(true);
                        } else {
                            self.input_area.input(ch);
                        }
                    }
                }
                UpdateResult {
                    cmd: Cmd::None,
                    pending_slash: None,
                }
            }
            Msg::Resize(_, _) => UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            },
            Msg::Tick => UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            },
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
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            };
        }

        // Dialog mode
        if self.active_dialog.is_some() {
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut d) = self.active_dialog {
                        d.select_prev();
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut d) = self.active_dialog {
                        d.select_next();
                    }
                }
                KeyCode::Enter => {
                    let selected = self.active_dialog.as_ref().and_then(|d| d.get_selected());
                    if let Some(idx) = selected {
                        if idx < self.dialog_model_keys.len() {
                            let model_key = self.dialog_model_keys[idx].clone();
                            self.input_queue.push_back(format!("/model {}", model_key));
                            self.active_dialog = None;
                            self.dialog_model_keys.clear();
                            return UpdateResult {
                                cmd: Cmd::None,
                                pending_slash: Some(format!("/model {}", model_key)),
                            };
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
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            };
        }

        // AskUserQuestion 交互模式（有选项列表）
        if let Some(ref state) = self.ask_user_state {
            let options_count = state.options.len();
            let multi_select = state.multi_select;

            match key.code {
                KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let cursor = if state.cursor == 0 {
                            options_count - 1
                        } else {
                            state.cursor - 1
                        };
                        self.ask_user_state.as_mut().unwrap().cursor = cursor;
                        let s = self.ask_user_state.as_ref().unwrap();
                        self.output_area.update_ask_user_options(
                            s.option_line_start,
                            &s.options,
                            s.cursor,
                            s.multi_select,
                            &s.selected,
                        );
                    }
                }
                KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let cursor = (state.cursor + 1) % options_count;
                        self.ask_user_state.as_mut().unwrap().cursor = cursor;
                        let s = self.ask_user_state.as_ref().unwrap();
                        self.output_area.update_ask_user_options(
                            s.option_line_start,
                            &s.options,
                            s.cursor,
                            s.multi_select,
                            &s.selected,
                        );
                    }
                }
                KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE && multi_select => {
                    let idx = state.cursor;
                    self.ask_user_state.as_mut().unwrap().selected[idx] = !state.selected[idx];
                    let s = self.ask_user_state.as_ref().unwrap();
                    self.output_area.update_ask_user_options(
                        s.option_line_start,
                        &s.options,
                        s.cursor,
                        s.multi_select,
                        &s.selected,
                    );
                }
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let state = self.ask_user_state.take().unwrap();
                    let answer = if multi_select {
                        // 多选：返回所有选中项的文本，逗号分隔
                        let selected: Vec<&str> = state
                            .selected
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| **s)
                            .map(|(i, _)| state.options[i].as_str())
                            .collect();
                        if selected.is_empty() {
                            // 没选任何项，返回光标所在项
                            state.options[state.cursor].clone()
                        } else {
                            selected.join(", ")
                        }
                    } else if options_count > 0 {
                        // 单选：返回光标所在项
                        state.options[state.cursor].clone()
                    } else {
                        // 无选项：取输入框文本
                        let text = self.input_area.get_text();
                        if text.is_empty() {
                            String::new()
                        } else {
                            text
                        }
                    };
                    if !answer.is_empty() {
                        self.output_area.push_user_message(&answer);
                    }
                    self.input_area.clear();
                    let _ = state.reply_tx.send(answer);
                    self.output_area.set_spinner_phase("Generating...");
                }
                KeyCode::Esc => {
                    let state = self.ask_user_state.take().unwrap();
                    self.input_area.clear();
                    let _ = state.reply_tx.send(String::new());
                    self.output_area.set_spinner_phase("Generating...");
                }
                _ => {
                    // 普通按键传递给 input_area（用于自由输入模式）
                    // Shift+Enter / Alt+Enter = 换行
                    if key.code == KeyCode::Enter
                        && key
                            .modifiers
                            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                    {
                        self.input_area.enter(true);
                    } else {
                        match (key.modifiers, key.code) {
                            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                    c.to_ascii_uppercase()
                                } else {
                                    c
                                };
                                self.input_area.input(ch);
                            }
                            (KeyModifiers::NONE, KeyCode::Backspace) => {
                                self.input_area.backspace();
                            }
                            (KeyModifiers::NONE, KeyCode::Left) => self.input_area.move_left(),
                            (KeyModifiers::NONE, KeyCode::Right) => self.input_area.move_right(),
                            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                                self.input_area.move_home()
                            }
                            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                                self.input_area.move_end()
                            }
                            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                                self.input_area.delete_word()
                            }
                            _ => {}
                        }
                    }
                }
            }
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            };
        }

        // AskUserQuestion 自由输入模式（无选项列表，等待 reply_tx）
        if self.ask_user_reply_tx.is_some() {
            match key.code {
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let text = self.input_area.get_text();
                    if !text.is_empty() {
                        if let Some(reply_tx) = self.ask_user_reply_tx.take() {
                            self.output_area.push_user_message(&text);
                            self.input_area.clear();
                            let _ = reply_tx.send(text);
                            self.output_area.set_spinner_phase("Generating...");
                        }
                    }
                    return UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    };
                }
                KeyCode::Esc => {
                    if let Some(reply_tx) = self.ask_user_reply_tx.take() {
                        self.input_area.clear();
                        let _ = reply_tx.send(String::new());
                        self.output_area.set_spinner_phase("Generating...");
                    }
                    return UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    };
                }
                // 其他按键传递给 input_area
                _ => {
                    // Shift+Enter / Alt+Enter = 换行
                    if key.code == KeyCode::Enter
                        && key
                            .modifiers
                            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                    {
                        self.input_area.enter(true);
                        return UpdateResult {
                            cmd: Cmd::None,
                            pending_slash: None,
                        };
                    }
                    match (key.modifiers, key.code) {
                        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                            let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                                c.to_ascii_uppercase()
                            } else {
                                c
                            };
                            self.input_area.input(ch);
                        }
                        (KeyModifiers::NONE, KeyCode::Backspace) => {
                            self.input_area.backspace();
                        }
                        (KeyModifiers::NONE, KeyCode::Left) => self.input_area.move_left(),
                        (KeyModifiers::NONE, KeyCode::Right) => self.input_area.move_right(),
                        (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.input_area.move_home(),
                        (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.input_area.move_end(),
                        (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                            self.input_area.delete_word()
                        }
                        _ => {}
                    }
                    return UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    };
                }
            }
        }

        // Shift+Enter / Alt+Enter = insert newline
        if (key.code == KeyCode::Enter || key.code == KeyCode::Char('\n'))
            && key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.input_area.enter(true);
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            };
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if *is_processing {
                    spawn_refs.interrupted.store(true, Ordering::Relaxed);
                    if let Ok(guard) = active_cancel.lock() {
                        if let Some(token) = guard.as_ref() {
                            token.cancel();
                        }
                    }
                    self.status_bar.set_warning("Interrupted");
                } else if self.input_area.is_showing_suggestions() {
                    self.input_area.clear_suggestions();
                } else {
                    let now = std::time::Instant::now();
                    if let Some(last) = self.last_ctrlc {
                        if now.duration_since(last).as_secs_f64() < 3.0 {
                            return UpdateResult {
                                cmd: Cmd::Quit,
                                pending_slash: None,
                            };
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
            (KeyModifiers::NONE, KeyCode::Esc) => {
                // Esc during processing: interrupt current LLM turn + tool calls
                spawn_refs.interrupted.store(true, Ordering::Relaxed);
                if let Ok(guard) = active_cancel.lock() {
                    if let Some(token) = guard.as_ref() {
                        token.cancel();
                    }
                }
                self.status_bar.set_warning("Interrupted");
            }
            (_, KeyCode::Enter) if *is_processing => {
                if !self.input_area.is_empty() {
                    let input = self.input_area.get_text();
                    self.input_area.add_history(&input);
                    self.input_area.clear();
                    self.input_queue.push_back(input.clone());
                    self.output_area.queued_messages.push(input);
                    let n = self.input_queue.len();
                    self.status_bar
                        .set_warning(&format!("{n} message(s) queued"));
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
            (KeyModifiers::SHIFT, KeyCode::Home) => {
                self.output_area.scroll_up(self.output_area.line_count())
            }
            (KeyModifiers::SHIFT, KeyCode::End) => self.output_area.scroll_to_bottom(),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.input_area.input(ch);
                if !*is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input_area.backspace();
                if !*is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.input_area.move_left();
                self.input_area.clear_suggestions();
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.input_area.move_right();
                self.input_area.clear_suggestions();
            }
            (KeyModifiers::NONE, KeyCode::Up) => self.input_area.move_up(),
            (KeyModifiers::NONE, KeyCode::Down) => self.input_area.move_down(),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.input_area.move_home(),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.input_area.move_end(),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.input_area.delete_word(),
            (KeyModifiers::CONTROL | KeyModifiers::SUPER, KeyCode::Char('v'))
                if !*is_processing && !self.just_pasted =>
            {
                self.just_pasted = true;
                let tx = ui_tx.clone();
                tokio::spawn(async move {
                    tx.send(UiEvent::SystemMessage(
                        "[reading clipboard image...]".to_string(),
                    ))
                    .await
                    .ok();
                    match crate::image::read_clipboard_image().await {
                        Ok(img) => {
                            let size = img.final_size;
                            tx.send(UiEvent::ClipboardImage(img)).await.ok();
                            tx.send(UiEvent::SystemMessage(format!(
                                "[clipboard image added ({} bytes). Type message to send.]",
                                size
                            )))
                            .await
                            .ok();
                        }
                        Err(e) => {
                            tx.send(UiEvent::SystemMessage(format!(
                                "No image in clipboard: {e}"
                            )))
                            .await
                            .ok();
                        }
                    }
                });
            }
            (KeyModifiers::NONE, KeyCode::End) => self.input_area.move_end(),
            _ => {}
        }

        UpdateResult {
            cmd: Cmd::None,
            pending_slash: None,
        }
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
            self.input_queue.push_back(input.clone());
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: Some(input),
            };
        }

        self.output_area.push_user_message(&input);
        self.input_area.add_history(&input);
        self.input_area.clear();

        let images: Vec<(String, String)> = self
            .pending_images
            .drain(..)
            .map(|img| (img.base64, img.media_type))
            .collect();
        if images.is_empty() {
            self.messages.push(Message::user(&input));
        } else {
            self.messages
                .push(Message::user_with_images(&input, images));
        }

        let spawn_ctx = self.build_spawn_context(ui_tx, active_cancel, spawn_refs);
        spawn_refs.interrupted.store(false, Ordering::Relaxed);
        self.active_tool_call_ids.clear();
        self.tool_call_active = false;
        self.output_area.start_spinner();
        self.output_area.set_spinner_phase("Thinking...");
        *is_processing = true;

        UpdateResult {
            cmd: Cmd::SpawnProcessing(spawn_ctx),
            pending_slash: None,
        }
    }

    /// Handle UI events from background processing
    fn update_ui(
        &mut self,
        ev: UiEvent,
        is_processing: &mut bool,
        _ui_tx: &mpsc::Sender<UiEvent>,
        _active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
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
            UiEvent::AgentProgress { tool_id, text } => {
                self.output_area.push_tool_progress(&tool_id, &text);
                self.output_area.start_spinner();
                self.output_area.set_spinner_phase("Agent working...");
            }
            UiEvent::Error(msg) => {
                log::debug!(
                    "[SPINNER] Error: tool_call_active {} -> false",
                    self.tool_call_active
                );
                let hook_runner = spawn_refs.hook_runner.clone();
                let msg_clone = msg.clone();
                tokio::spawn(async move {
                    let _ = hook_runner.on_notification(&msg_clone, "error").await;
                });
                self.output_area.push_error(&msg);
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                *is_processing = false;
            }
            UiEvent::Cancelled => {
                self.output_area.push_cancelled();
                self.output_area.stop_spinner();
                self.tool_call_active = false;
                self.active_tool_call_ids.clear();
                *is_processing = false;
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
                let hook_runner = spawn_refs.hook_runner.clone();
                let msg_clone = msg.clone();
                tokio::spawn(async move {
                    let _ = hook_runner
                        .on_notification(&msg_clone, "system_message")
                        .await;
                });
                self.output_area.push_system(&msg);
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
                    self.ask_user_state = Some(super::AskUserState {
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
                    let hook_runner = spawn_refs.hook_runner.clone();
                    let msg_clone = msg.clone();
                    tokio::spawn(async move {
                        let _ = hook_runner
                            .on_notification(&msg_clone, "system_message")
                            .await;
                    });
                    self.output_area.push_system(msg);
                }
                if let Some(ref ctx) = additional_context {
                    self.output_area
                        .push_system(&format!("[Additional Context] {ctx}"));
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
                *is_processing = false;
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
                *is_processing = false;
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
            queue_request_tx: ui_tx.clone(),
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
            session_reminders: spawn_refs.session_reminders.clone(),
            agent_runner: spawn_refs.agent_runner.clone(),
            allow_all: spawn_refs.allow_all,
            interrupted: spawn_refs.interrupted.clone(),
            cancel,
            task_store: spawn_refs.task_store.clone(),
            max_tool_concurrency: spawn_refs.max_tool_concurrency,
            max_agent_concurrency: spawn_refs.max_agent_concurrency,
            agent_semaphore: spawn_refs.agent_semaphore.clone(),
            hook_runner: spawn_refs.hook_runner.clone(),
            memory_config: spawn_refs.memory_config.clone(),
        }
    }
}

/// Type alias so update.rs can use `App` without circular path
use super::App;

use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::processing::SpawnContextRefs;
use crate::tui::app::{App, UiEvent};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    pub(super) fn update_key(
        &mut self,
        key: crossterm::event::KeyEvent,
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

        if let Some(result) = self.update_ask_user_key(key) {
            return result;
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
                if self.is_processing {
                    spawn_refs.interrupted.store(true, Ordering::Relaxed);
                    if let Ok(guard) = active_cancel.lock() {
                        if let Some(token) = guard.as_ref() {
                            token.cancel();
                        }
                    }
                    self.status_bar.set_warning("Interrupted");
                } else if self.input_area.is_showing_suggestions() {
                    self.input_area.clear_suggestions();
                } else if !self.input_area.is_empty() {
                    // 第一次 Ctrl+C：input 非空时清空 input area
                    self.input_area.clear();
                    self.status_bar.set_warning("Input cleared (Ctrl+C again to exit)");
                    self.last_ctrlc = Some(std::time::Instant::now());
                } else {
                    // input 为空：两段式退出（5 秒超时）
                    let now = std::time::Instant::now();
                    if let Some(last) = self.last_ctrlc {
                        if now.duration_since(last).as_secs_f64() < 5.0 {
                            return UpdateResult {
                                cmd: Cmd::Quit,
                                pending_slash: None,
                            };
                        }
                    }
                    self.last_ctrlc = Some(now);
                    self.status_bar.set_warning("Press Ctrl+C again to exit");
                }
            }
            (KeyModifiers::NONE, KeyCode::Tab) if !self.is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Esc) if !self.is_processing => {
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
            (_, KeyCode::Enter) if self.is_processing => {
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
            (_, KeyCode::Enter) if !self.is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else if !self.input_area.is_empty() {
                    return self.update_enter(ui_tx, active_cancel, spawn_refs);
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
                if !self.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input_area.backspace();
                if !self.is_processing {
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
                if !self.is_processing && !self.just_pasted =>
            {
                self.just_pasted = true;
                self.output_area.push_system("[reading clipboard image...]");
                return UpdateResult {
                    cmd: Cmd::ReadClipboardImage,
                    pending_slash: None,
                };
            }
            (KeyModifiers::NONE, KeyCode::End) => self.input_area.move_end(),
            _ => {}
        }

        UpdateResult {
            cmd: Cmd::None,
            pending_slash: None,
        }
    }
}

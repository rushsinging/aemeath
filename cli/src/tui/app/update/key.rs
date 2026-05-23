use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::processing::SpawnContextRefs;
use crate::tui::app::{App, UiEvent};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Ctrl+C 在非 processing、非 suggestions 状态下的动作。
/// 提取为纯函数以便单元测试。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CtrlCAction {
    /// 清空 input area 内容
    ClearInput,
    /// 显示「再按一次退出」提示
    WarnExit,
    /// 退出 TUI
    Quit,
}

/// Ctrl+C 两段式退出超时（秒）
pub(crate) const CTRL_C_TIMEOUT_SECS: f64 = 3.0;

/// 根据 input 是否为空和上次 Ctrl+C 时间戳决定动作。
fn ctrlc_action(input_empty: bool, last_ctrlc: Option<std::time::Instant>) -> CtrlCAction {
    if !input_empty {
        CtrlCAction::ClearInput
    } else {
        let now = std::time::Instant::now();
        if let Some(last) = last_ctrlc {
            if now.duration_since(last).as_secs_f64() < CTRL_C_TIMEOUT_SECS {
                CtrlCAction::Quit
            } else {
                CtrlCAction::WarnExit
            }
        } else {
            CtrlCAction::WarnExit
        }
    }
}

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
                } else {
                    match ctrlc_action(self.input_area.is_empty(), self.last_ctrlc) {
                        CtrlCAction::ClearInput => {
                            self.input_area.clear();
                            self.status_bar
                                .set_warning("Input cleared (Ctrl+C again to exit)");
                            self.last_ctrlc = Some(std::time::Instant::now());
                        }
                        CtrlCAction::WarnExit => {
                            self.last_ctrlc = Some(std::time::Instant::now());
                            self.status_bar.set_warning("Press Ctrl+C again to exit");
                        }
                        CtrlCAction::Quit => {
                            return UpdateResult {
                                cmd: Cmd::Quit,
                                pending_slash: None,
                            };
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrlc_action_input_nonempty_clears() {
        // input 非空时，无论 last_ctrlc 状态如何，都应清空 input
        assert_eq!(
            ctrlc_action(false, None),
            CtrlCAction::ClearInput,
            "非空 input、无上次记录 → ClearInput"
        );
        assert_eq!(
            ctrlc_action(false, Some(std::time::Instant::now())),
            CtrlCAction::ClearInput,
            "非空 input、有上次记录 → ClearInput"
        );
    }

    #[test]
    fn test_ctrlc_action_empty_first_press_warns() {
        // 空 input、首次按 Ctrl+C → 提示退出
        assert_eq!(
            ctrlc_action(true, None),
            CtrlCAction::WarnExit,
            "空 input、无上次记录 → WarnExit"
        );
    }

    #[test]
    fn test_ctrlc_action_empty_quick_second_press_quits() {
        // 空 input、上次在超时窗口内 → 退出
        let recent = std::time::Instant::now();
        assert_eq!(
            ctrlc_action(true, Some(recent)),
            CtrlCAction::Quit,
            "空 input、超时窗口内 → Quit"
        );
    }

    #[test]
    fn test_ctrlc_action_empty_expired_second_press_warns() {
        // 空 input、上次已过期 → 重新提示
        let expired = std::time::Instant::now() - std::time::Duration::from_secs(4);
        assert_eq!(
            ctrlc_action(true, Some(expired)),
            CtrlCAction::WarnExit,
            "空 input、超时已过 → WarnExit"
        );
    }

    #[test]
    fn test_ctrlc_action_boundary_timeout() {
        // 刚好在超时边界上（略小于 3 秒 → Quit）
        let just_inside = std::time::Instant::now() - std::time::Duration::from_millis(2900);
        assert_eq!(
            ctrlc_action(true, Some(just_inside)),
            CtrlCAction::Quit,
            "2.9 秒前 → Quit"
        );

        // 刚好超出（略大于 3 秒 → WarnExit）
        let just_outside = std::time::Instant::now() - std::time::Duration::from_millis(3100);
        assert_eq!(
            ctrlc_action(true, Some(just_outside)),
            CtrlCAction::WarnExit,
            "3.1 秒前 → WarnExit"
        );
    }
}

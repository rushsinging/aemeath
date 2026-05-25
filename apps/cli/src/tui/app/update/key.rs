use super::key_nav::handle_dialog_key;
use super::key_scroll::handle_scroll_key;
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

        if self.layout.active_dialog.is_some() {
            return handle_dialog_key(self, key).unwrap_or(UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            });
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
                if self.chat.is_processing {
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
                    match ctrlc_action(self.input_area.is_empty(), self.layout.last_ctrlc) {
                        CtrlCAction::ClearInput => {
                            self.input_area.clear();
                            self.status_bar
                                .set_warning("Input cleared (Ctrl+C again to exit)");
                            self.layout.last_ctrlc = Some(std::time::Instant::now());
                        }
                        CtrlCAction::WarnExit => {
                            self.layout.last_ctrlc = Some(std::time::Instant::now());
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
            (KeyModifiers::NONE, KeyCode::Tab) if !self.chat.is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Esc) if !self.chat.is_processing => {
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
            (_, KeyCode::Enter) if self.chat.is_processing => {
                if !self.input_area.is_empty() {
                    let input = self.input_area.get_text();
                    self.input_area.add_history(&input);
                    self.input_area.clear();
                    self.input.input_queue.push_back(input.clone());
                    self.output_area.queued_messages.push(input);
                    let n = self.input.input_queue.len();
                    self.status_bar
                        .set_warning(&format!("{n} message(s) queued"));
                }
            }
            (_, KeyCode::Enter) if !self.chat.is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else if !self.input_area.is_empty() {
                    return self.update_enter(ui_tx, active_cancel, spawn_refs);
                }
            }
            _ if handle_scroll_key(self, key, key.modifiers) => {}
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.input_area.input(ch);
                if !self.chat.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input_area.backspace();
                if !self.chat.is_processing {
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
                if !self.chat.is_processing && !self.input.just_pasted =>
            {
                self.input.just_pasted = true;
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
        assert_eq!(ctrlc_action(false, None), CtrlCAction::ClearInput);
        assert_eq!(
            ctrlc_action(false, Some(std::time::Instant::now())),
            CtrlCAction::ClearInput
        );
    }

    #[test]
    fn test_ctrlc_action_empty_first_press_warns() {
        assert_eq!(ctrlc_action(true, None), CtrlCAction::WarnExit);
    }

    #[test]
    fn test_ctrlc_action_empty_quick_second_press_quits() {
        let recent = std::time::Instant::now();
        assert_eq!(ctrlc_action(true, Some(recent)), CtrlCAction::Quit);
    }

    #[test]
    fn test_ctrlc_action_empty_expired_second_press_warns() {
        let expired = std::time::Instant::now() - std::time::Duration::from_secs(4);
        assert_eq!(ctrlc_action(true, Some(expired)), CtrlCAction::WarnExit);
    }

    #[test]
    fn test_ctrlc_action_boundary_timeout() {
        let just_inside = std::time::Instant::now() - std::time::Duration::from_millis(2900);
        assert_eq!(ctrlc_action(true, Some(just_inside)), CtrlCAction::Quit);

        let just_outside = std::time::Instant::now() - std::time::Duration::from_millis(3100);
        assert_eq!(ctrlc_action(true, Some(just_outside)), CtrlCAction::WarnExit);
    }
}

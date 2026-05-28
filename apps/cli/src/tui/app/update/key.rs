use super::key_nav::handle_dialog_key;
use super::key_scroll::handle_scroll_key;
use super::UpdateResult;
use crate::tui::adapter::input_widget::apply_input_changes_to_widget;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::input::intent::InputIntent;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

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
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        if key.kind != KeyEventKind::Press {
            return UpdateResult::none();
        }

        if self.layout.has_active_dialog() {
            return handle_dialog_key(self, key).unwrap_or_else(UpdateResult::none);
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
            // 同步模型状态：enter 直接修改 textarea 未走模型（同 #77/#78），
            // 下次按键将触发 model.insert → TextChanged → set_text，
            // 用旧文本覆盖 textarea 中的换行后内容。
            let text = self.input_area.get_text();
            self.model.input.document.clear();
            self.model.input.document.insert_text(&text);
            return UpdateResult::none();
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.chat.is_processing {
                    if let Some(agent_client) = &spawn_refs.agent_client {
                        agent_client.cancel();
                    }
                    self.status_bar.set_warning("Interrupted");
                } else if self.input_area.is_showing_suggestions() {
                    self.input_area.clear_suggestions();
                } else {
                    match ctrlc_action(self.input_area.is_empty(), self.layout.last_ctrlc) {
                        CtrlCAction::ClearInput => {
                            let changes = self.model.input.apply(InputIntent::Clear);
                            apply_input_changes_to_widget(
                                &mut self.input_area,
                                &mut self.status_bar,
                                &changes,
                            );
                            self.status_bar
                                .set_warning("Input cleared (Ctrl+C again to exit)");
                            self.layout.mark_ctrlc_now();
                        }
                        CtrlCAction::WarnExit => {
                            self.layout.mark_ctrlc_now();
                            self.status_bar.set_warning("Press Ctrl+C again to exit");
                        }
                        CtrlCAction::Quit => {
                            return UpdateResult::one(Effect::QuitApplication);
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
                if let Some(agent_client) = &spawn_refs.agent_client {
                    agent_client.cancel();
                }
                self.status_bar.set_warning("Interrupted");
            }
            (_, KeyCode::Enter) if self.chat.is_processing => {
                if !self.input_area.is_empty() {
                    let input = self.input_area.get_text();
                    let changes = self.model.input.apply(InputIntent::Submit);
                    apply_input_changes_to_widget(
                        &mut self.input_area,
                        &mut self.status_bar,
                        &changes,
                    );
                    let n = self.input.push_queue(input.clone());
                    self.output_area.queued_messages.push(input);
                    self.status_bar
                        .set_warning(&format!("{n} message(s) queued"));
                }
            }
            (_, KeyCode::Enter) if !self.chat.is_processing => {
                if self.input_area.is_showing_suggestions() {
                    self.apply_current_suggestion();
                } else if !self.input_area.is_empty() {
                    return self.update_enter(ui_tx, spawn_refs);
                }
            }
            _ if handle_scroll_key(self, key, key.modifiers) => {}
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                let changes = self.model.input.apply(InputIntent::InsertChar(ch));
                apply_input_changes_to_widget(&mut self.input_area, &mut self.status_bar, &changes);
                if !self.chat.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                let changes = self.model.input.apply(InputIntent::DeleteBackward);
                apply_input_changes_to_widget(&mut self.input_area, &mut self.status_bar, &changes);
                if !self.chat.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.input_area.move_left();
                self.input_area.clear_suggestions();
                // 光标仅在 textarea 内移动，模型 cursor 未更新。
                // 后续 InsertChar 以模型旧光标插入，表现错位。
                self.model
                    .input
                    .document
                    .set_cursor_col(self.input_area.cursor_position().1);
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.input_area.move_right();
                self.input_area.clear_suggestions();
                self.model
                    .input
                    .document
                    .set_cursor_col(self.input_area.cursor_position().1);
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                self.input_area.move_up();
                // move_up 可能触发历史翻看，会改变 textarea 文本，
                // 需要同步模型（同 #77/#78/#79 系列）
                let text = self.input_area.get_text();
                self.model.input.document.clear();
                self.model.input.document.insert_text(&text);
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                self.input_area.move_down();
                let text = self.input_area.get_text();
                self.model.input.document.clear();
                self.model.input.document.insert_text(&text);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.input_area.move_home();
                self.model
                    .input
                    .document
                    .set_cursor_col(self.input_area.cursor_position().1);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.input_area.move_end();
                self.model
                    .input
                    .document
                    .set_cursor_col(self.input_area.cursor_position().1);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.input_area.delete_word();
                // delete_word 修改 textarea 文本，需要同步模型
                let text = self.input_area.get_text();
                self.model.input.document.clear();
                self.model.input.document.insert_text(&text);
            }
            (KeyModifiers::CONTROL | KeyModifiers::SUPER, KeyCode::Char('v'))
                if !self.chat.is_processing && !self.input.just_pasted =>
            {
                self.input.just_pasted = true;
                self.output_area.push_system("[reading clipboard image...]");
                return UpdateResult::one(Effect::ReadClipboardImage);
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.input_area.move_end();
                self.model
                    .input
                    .document
                    .set_cursor_col(self.input_area.cursor_position().1);
            }
            _ => {}
        }

        UpdateResult::none()
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
        assert_eq!(
            ctrlc_action(true, Some(just_outside)),
            CtrlCAction::WarnExit
        );
    }
}

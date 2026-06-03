use super::key_nav::handle_dialog_key;
use super::key_scroll::handle_scroll_key;
use super::UpdateResult;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::input::intent::InputIntent;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

/// Ctrl+C 在非 processing、非 completion 状态下的动作。
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
    pub(crate) fn handle_input_intent(&mut self, intent: InputIntent) {
        let changes = self.model.input.apply(intent);
        crate::tui::adapter::input_widget::apply_input_changes_to_widget(
            &mut self.input_area,
            &mut self.status_bar,
            &changes,
        );
    }

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
            self.handle_input_intent(InputIntent::InsertNewline);
            return UpdateResult::none();
        }

        let completion_visible = self.model.input.completion.visible;

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.chat.is_processing {
                    if let Some(agent_client) = &spawn_refs.agent_client {
                        agent_client.cancel();
                    }
                    self.status_bar.set_warning("Interrupted");
                } else if completion_visible {
                    self.handle_input_intent(InputIntent::SetCompletions {
                        query: String::new(),
                        items: Vec::new(),
                    });
                } else {
                    match ctrlc_action(self.model.input.document.is_empty(), self.layout.last_ctrlc)
                    {
                        CtrlCAction::ClearInput => {
                            self.handle_input_intent(InputIntent::Clear);
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
                if completion_visible {
                    self.apply_current_suggestion();
                } else {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Esc) if !self.chat.is_processing => {
                if completion_visible {
                    self.handle_input_intent(InputIntent::SetCompletions {
                        query: String::new(),
                        items: Vec::new(),
                    });
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
                if !self.model.input.document.is_empty() {
                    let changes = self.model.input.apply(InputIntent::Submit);
                    crate::tui::adapter::input_widget::apply_input_changes_to_widget(
                        &mut self.input_area,
                        &mut self.status_bar,
                        &changes,
                    );
                    let input = changes
                        .iter()
                        .find_map(|change| {
                            if let crate::tui::model::input::change::InputChange::Submitted {
                                submission,
                            } = change
                            {
                                Some(submission.text.clone())
                            } else {
                                None
                            }
                        })
                        .unwrap_or_default();
                    let event = sdk::ChatInputEvent::classify_text(input.clone(), Vec::new());
                    // 入队即时显示「排队中」块（QueuedUserMessage），由 MessagesSync drain 时清理。
                    self.enqueue_submission_echo(input.clone());
                    self.status_bar.set_warning("message event queued");
                    return UpdateResult::one(Effect::SendChatInputEvent { event });
                }
            }
            (_, KeyCode::Enter) if !self.chat.is_processing => {
                if completion_visible {
                    self.apply_current_suggestion();
                } else if !self.model.input.document.is_empty() {
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
                self.handle_input_intent(InputIntent::InsertChar(ch));
                if !self.chat.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.handle_input_intent(InputIntent::DeleteBackward);
                if !self.chat.is_processing {
                    self.update_suggestions();
                }
            }
            (KeyModifiers::NONE, KeyCode::Left) => {
                self.handle_input_intent(InputIntent::MoveCursorLeft);
            }
            (KeyModifiers::NONE, KeyCode::Right) => {
                self.handle_input_intent(InputIntent::MoveCursorRight);
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                if completion_visible {
                    self.handle_input_intent(InputIntent::SelectCompletionPrevious);
                } else {
                    self.handle_input_intent(InputIntent::MoveCursorUp);
                }
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                if completion_visible {
                    self.handle_input_intent(InputIntent::SelectCompletionNext);
                } else {
                    self.handle_input_intent(InputIntent::MoveCursorDown);
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.handle_input_intent(InputIntent::MoveCursorHome);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.handle_input_intent(InputIntent::MoveCursorEnd);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
                self.handle_input_intent(InputIntent::DeleteWordBeforeCursor);
            }
            (KeyModifiers::CONTROL | KeyModifiers::SUPER, KeyCode::Char('v'))
                if !self.chat.is_processing && !self.input.just_pasted =>
            {
                self.input.just_pasted = true;
                self.append_system_notice("[reading clipboard image...]");
                return UpdateResult::one(Effect::ReadClipboardImage);
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.handle_input_intent(InputIntent::MoveCursorEnd);
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

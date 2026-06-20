use super::key_nav::handle_dialog_key;
use super::key_scroll::handle_scroll_key;
use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::input::change::submitted_submission_from_changes;
use crate::tui::model::input::intent::InputIntent;
use crate::tui::model::runtime::intent::RuntimeIntent;
use crate::tui::model::runtime::status_notice::StatusNotice;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

/// Ctrl+C 动作。
/// 提取为纯函数以便单元测试。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CtrlCAction {
    /// 清空 input area 内容
    ClearInput,
    /// 显示「再按一次退出」提示
    WarnExit,
    /// 退出 TUI
    Quit,
    /// 请求取消当前处理
    RequestCancel,
    /// 取消中再次按下时强制退出
    ForceQuit,
}

/// Ctrl+C 两段式退出超时（秒）
pub(crate) const CTRL_C_TIMEOUT_SECS: f64 = 3.0;

/// 根据 input 是否为空、上次 Ctrl+C 时间戳和处理生命周期状态决定动作。
fn ctrlc_action(
    input_empty: bool,
    last_ctrlc: Option<std::time::Instant>,
    is_processing: bool,
    is_cancelling: bool,
) -> CtrlCAction {
    if is_processing && is_cancelling {
        CtrlCAction::ForceQuit
    } else if is_processing {
        CtrlCAction::RequestCancel
    } else if !input_empty {
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
        // Input changes update the model only; render paths read model-derived view state directly.
        let _changes = self.model.input.apply(intent);
    }

    pub(super) fn update_key(
        &mut self,
        key: crossterm::event::KeyEvent,
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
                match ctrlc_action(
                    self.model.input.document.is_empty(),
                    self.layout.last_ctrlc,
                    self.chat.is_processing,
                    self.chat.is_cancelling,
                ) {
                    CtrlCAction::RequestCancel => {
                        self.chat.start_cancelling();
                        self.layout.mark_ctrlc_now();
                        return UpdateResult::one(Effect::CancelAgentChat);
                    }
                    CtrlCAction::ForceQuit => {
                        return UpdateResult::one(Effect::QuitApplication);
                    }
                    CtrlCAction::ClearInput => {
                        self.handle_input_intent(InputIntent::Clear);
                        self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                            StatusNotice::warning("Input cleared (Ctrl+C again to exit)"),
                        ));
                        self.layout.mark_ctrlc_now();
                    }
                    CtrlCAction::WarnExit => {
                        if completion_visible {
                            self.handle_input_intent(InputIntent::SetCompletions {
                                query: String::new(),
                                items: Vec::new(),
                            });
                        } else {
                            self.layout.mark_ctrlc_now();
                            self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                                StatusNotice::warning("Press Ctrl+C again to exit"),
                            ));
                        }
                    }
                    CtrlCAction::Quit => {
                        return UpdateResult::one(Effect::QuitApplication);
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
                self.model
                    .runtime
                    .apply(RuntimeIntent::SetStatusNotice(StatusNotice::warning(
                        "Interrupted",
                    )));
            }
            (_, KeyCode::Enter) if self.chat.is_processing => {
                if !self.model.input.document.is_empty() {
                    let changes = self.model.input.apply(InputIntent::Submit);
                    let Some(submission) = submitted_submission_from_changes(&changes) else {
                        return UpdateResult::none();
                    };
                    // 忙时 slash/control command 保持现有 mid-turn 行为：作为
                    // ControlCommand 事件入通道，永不作为 user message 发给 LLM（A3/#391）。
                    if submission.text.starts_with('/') {
                        let event = sdk::ChatInputEvent::ControlCommand {
                            raw: submission.text.clone(),
                        };
                        self.input.push_queue(submission.text.clone());
                        self.enqueue_submission_echo(
                            sdk::InputId::new_v7(),
                            submission.display_text,
                        );
                        self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                            StatusNotice::warning("message event queued"),
                        ));
                        return UpdateResult::one(Effect::SendChatInputEvent { event });
                    }
                    // 忙时普通消息：与首条提交统一经事件通道发 UserMessage。
                    self.model.runtime.apply(RuntimeIntent::SetStatusNotice(
                        StatusNotice::warning("message event queued"),
                    ));
                    return self.submit_user_input_event(submission);
                }
            }
            (_, KeyCode::Enter) if !self.chat.is_processing => {
                if completion_visible {
                    self.apply_current_suggestion();
                } else if !self.model.input.document.is_empty() {
                    return self.update_enter();
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
                } else if !self.input.input_queue.is_empty() {
                    // input queue 非空：全部恢复到 input area（\n 连接），清空 queue 和显示块
                    let queued = self.input.drain_queue();
                    self.handle_input_intent(InputIntent::ReplaceText(queued.join("\n")));
                    self.clear_queued_submission_echo();
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
#[path = "key_tests.rs"]
mod key_tests;

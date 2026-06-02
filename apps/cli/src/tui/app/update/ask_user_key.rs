use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    pub(super) fn update_ask_user_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        // AskUserQuestion 交互模式（有选项列表）
        if let Some(ref state) = self.input.ask_user_state {
            // 导航/勾选/子态的可变真相在 ConversationModel 的 AskUser 块
            let snapshot = self.model.conversation.ask_user_snapshot();
            let chat_input_active = snapshot
                .as_ref()
                .map(|s| s.chat_input_active)
                .unwrap_or(false);
            // Chat-input sub-mode: user is typing free text via "Chat about this..."
            if chat_input_active {
                return self.update_ask_user_chat_input_key(key);
            }

            let options_count = state.options.len();
            let multi_select = state.multi_select;
            let cursor = snapshot.as_ref().map(|s| s.cursor).unwrap_or(0);

            match key.code {
                KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let next = if cursor == 0 {
                            options_count - 1
                        } else {
                            cursor - 1
                        };
                        self.set_ask_user_cursor(next);
                    }
                }
                KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let next = (cursor + 1) % options_count;
                        self.set_ask_user_cursor(next);
                    }
                }
                KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE && multi_select => {
                    // 内建选项不可勾选（toggle 内部已校验，此处保持早返回行为一致）
                    if cursor >= state.llm_option_count {
                        return Some(UpdateResult::none());
                    }
                    self.toggle_ask_user_selected(cursor);
                }
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let state = self.input.ask_user_state.take().unwrap();
                    let selected = snapshot.map(|s| s.selected).unwrap_or_default();
                    let cursor_title = state
                        .options
                        .get(cursor)
                        .map(|o| o.title.as_str())
                        .unwrap_or("");

                    let answer = if cursor_title == crate::tui::app::state::BUILTIN_OPTION_CHAT {
                        // "Type something...": switch to chat input sub-mode
                        self.input.ask_user_state = Some(state);
                        self.set_ask_user_chat_input(true);
                        self.handle_input_intent(
                            crate::tui::model::input::intent::InputIntent::Clear,
                        );
                        return Some(UpdateResult::none());
                    } else if multi_select {
                        // Multi-select: return selected items, comma-separated
                        let chosen: Vec<String> = selected
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| **s)
                            .filter_map(|(i, _)| state.options.get(i).map(|o| o.title.clone()))
                            .collect();
                        if chosen.is_empty() {
                            state
                                .options
                                .get(cursor)
                                .map(|o| o.title.clone())
                                .unwrap_or_default()
                        } else {
                            chosen.join(", ")
                        }
                    } else if options_count > 0 {
                        // Single select: return cursor item title
                        state
                            .options
                            .get(cursor)
                            .map(|o| o.title.clone())
                            .unwrap_or_default()
                    } else {
                        let text = self.model.input.document.buffer.clone();
                        if text.is_empty() {
                            String::new()
                        } else {
                            text
                        }
                    };

                    self.dismiss_ask_user_block();
                    if !answer.is_empty() {
                        self.append_user_echo(answer.clone());
                    }
                    self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
                    let _ = state.reply_tx.send(answer);
                    self.spinner_phase(SpinnerPhase::Generating);
                }
                KeyCode::Esc => {
                    let state = self.input.ask_user_state.take().unwrap();
                    self.dismiss_ask_user_block();
                    self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
                    let _ = state.reply_tx.send(String::new());
                    self.spinner_phase(SpinnerPhase::Generating);
                }
                _ => {
                    // 选项模式下忽略其他按键
                }
            }
            return Some(UpdateResult::none());
        }

        // AskUserQuestion 自由输入模式（无选项列表，等待 reply_tx）
        if self.input.ask_user_reply_tx.is_some() {
            match key.code {
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let text = self.model.input.document.buffer.clone();
                    if !text.is_empty() {
                        if let Some(reply_tx) = self.input.ask_user_reply_tx.take() {
                            self.dismiss_ask_user_block();
                            self.append_user_echo(text.clone());
                            self.handle_input_intent(
                                crate::tui::model::input::intent::InputIntent::Clear,
                            );
                            let _ = reply_tx.send(text);
                            self.spinner_phase(SpinnerPhase::Generating);
                        }
                    }
                    return Some(UpdateResult::none());
                }
                KeyCode::Esc => {
                    if let Some(reply_tx) = self.input.ask_user_reply_tx.take() {
                        self.dismiss_ask_user_block();
                        self.handle_input_intent(
                            crate::tui::model::input::intent::InputIntent::Clear,
                        );
                        let _ = reply_tx.send(String::new());
                        self.spinner_phase(SpinnerPhase::Generating);
                    }
                    return Some(UpdateResult::none());
                }
                // 其他按键传递给 input_area
                _ => {
                    self.update_ask_user_input_key(key);
                    return Some(UpdateResult::none());
                }
            }
        }

        None
    }

    /// Handle keys in the "Type something..." free-text sub-mode.
    fn update_ask_user_chat_input_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        match key.code {
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                let text = self
                    .model
                    .conversation
                    .ask_user_chat_text()
                    .unwrap_or_default();
                if !text.is_empty() {
                    let state = self.input.ask_user_state.take().unwrap();
                    self.dismiss_ask_user_block();
                    self.append_user_echo(text.clone());
                    let _ = state.reply_tx.send(text);
                    self.spinner_phase(SpinnerPhase::Generating);
                }
            }
            KeyCode::Esc => {
                // Return to option list without submitting
                self.set_ask_user_chat_input(false);
            }
            KeyCode::Backspace => {
                self.model
                    .conversation
                    .apply(ConversationIntent::DeleteAskUserChatChar);
                self.refresh_output_widget_from_model();
            }
            KeyCode::Char(c) => {
                self.model
                    .conversation
                    .apply(ConversationIntent::AppendAskUserChatChar { ch: c });
                self.refresh_output_widget_from_model();
            }
            KeyCode::Up => {
                // Move cursor back to last option
                let snapshot = self.model.conversation.ask_user_snapshot();
                if let Some(snap) = snapshot {
                    let options_count = snap.cursor;
                    if options_count > 0 {
                        self.set_ask_user_chat_input(false);
                        self.set_ask_user_cursor(options_count - 1);
                    }
                }
            }
            _ => {}
        }
        Some(UpdateResult::none())
    }

    pub(super) fn update_ask_user_input_key(&mut self, key: crossterm::event::KeyEvent) {
        // Shift+Enter / Alt+Enter = 换行
        if key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertNewline);
            return;
        }
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::InsertChar(ch),
                );
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::DeleteBackward,
                );
            }
            (KeyModifiers::NONE, KeyCode::Left) => self
                .handle_input_intent(crate::tui::model::input::intent::InputIntent::MoveCursorLeft),
            (KeyModifiers::NONE, KeyCode::Right) => self.handle_input_intent(
                crate::tui::model::input::intent::InputIntent::MoveCursorRight,
            ),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self
                .handle_input_intent(crate::tui::model::input::intent::InputIntent::MoveCursorHome),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self
                .handle_input_intent(crate::tui::model::input::intent::InputIntent::MoveCursorEnd),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.handle_input_intent(
                crate::tui::model::input::intent::InputIntent::DeleteWordBeforeCursor,
            ),
            _ => {}
        }
    }
}

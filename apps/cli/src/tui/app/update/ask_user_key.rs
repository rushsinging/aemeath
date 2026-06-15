use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::model::conversation::block::AskUserPhase;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::runtime::spinner::SpinnerPhase;
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    pub(super) fn update_ask_user_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        // 批量 AskUserQuestion 交互模式
        if let Some(ref _state) = self.input.ask_user_state {
            let snapshot = self.model.conversation.ask_user_snapshot();

            // Chat-input 子态：用户在 Type something 中输入自由文本
            let chat_input_active = snapshot
                .as_ref()
                .map(|s| s.chat_input_active)
                .unwrap_or(false);
            if chat_input_active {
                return self.update_ask_user_chat_input_key(key);
            }

            let phase = snapshot
                .as_ref()
                .map(|s| s.phase)
                .unwrap_or(AskUserPhase::Answering);

            return match phase {
                AskUserPhase::Answering => self.update_ask_user_answering_key(key, &snapshot),
                AskUserPhase::Confirming => self.update_ask_user_confirming_key(key),
            };
        } else if self.input.ask_user_reply_tx.is_some() {
            // AskUserQuestion 自由输入模式（无选项列表，等待 reply_tx）
            match key.code {
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let text = self.model.input.document.buffer.clone();
                    if !text.is_empty() {
                        if let Some(reply_tx) = self.input.ask_user_reply_tx.take() {
                            self.model.conversation.apply(
                                ConversationIntent::AnswerCurrentAskUser {
                                    answer: text.clone(),
                                },
                            );
                            self.mark_output_dirty();
                            self.handle_input_intent(
                                crate::tui::model::input::intent::InputIntent::Clear,
                            );
                            let _ = reply_tx.send(vec![text]);
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
                        let _ = reply_tx.send(vec![String::new()]);
                        self.spinner_phase(SpinnerPhase::Generating);
                    }
                    return Some(UpdateResult::none());
                }
                _ => {
                    self.update_ask_user_input_key(key);
                    return Some(UpdateResult::none());
                }
            }
        }

        None
    }

    /// Answering 阶段键盘处理：选项导航 + 勾选 + 提交单题答案。
    fn update_ask_user_answering_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        snapshot: &Option<crate::tui::model::conversation::ask_user::AskUserSnapshot>,
    ) -> Option<UpdateResult> {
        let snap = match snapshot.as_ref() {
            Some(s) => s,
            None => return Some(UpdateResult::none()),
        };
        let options_count = snap.options_count;
        let multi_select = snap.multi_select;
        let cursor = snap.cursor;
        let llm_option_count = snap.llm_option_count;

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
                if cursor >= llm_option_count {
                    return Some(UpdateResult::none());
                }
                self.toggle_ask_user_selected(cursor);
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                let state = self.input.ask_user_state.as_ref().unwrap();
                let active_index = snap.active_index;
                let active_item = &state.items[active_index];

                let answer = if cursor >= llm_option_count && options_count > 0 {
                    // "Type something..." 被选中 → 切换到自由输入子态
                    self.set_ask_user_chat_input(true);
                    self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
                    return Some(UpdateResult::none());
                } else if multi_select {
                    // Multi-select: 返回已选项目，逗号分隔
                    let chosen: Vec<String> = snap
                        .selected
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| **s)
                        .filter_map(|(i, _)| active_item.options.get(i).map(|o| o.title.clone()))
                        .collect();
                    if chosen.is_empty() {
                        active_item
                            .options
                            .get(cursor)
                            .map(|o| o.title.clone())
                            .unwrap_or_default()
                    } else {
                        chosen.join(", ")
                    }
                } else if options_count > 0 {
                    // Single select: cursor 对应选项
                    active_item
                        .options
                        .get(cursor)
                        .map(|o| o.title.clone())
                        .unwrap_or_default()
                } else {
                    // 无选项自由输入模式
                    self.model.input.document.buffer.clone()
                };

                let final_answer = if answer.is_empty() {
                    // 使用 default
                    active_item.default.clone().unwrap_or_default()
                } else {
                    answer
                };

                self.model
                    .conversation
                    .apply(ConversationIntent::AnswerCurrentAskUser {
                        answer: final_answer,
                    });
                self.mark_output_dirty();
                self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
            }
            KeyCode::Esc => {
                // 取消整个 batch
                self.cancel_ask_user_batch();
            }
            _ => {}
        }
        Some(UpdateResult::none())
    }

    /// Confirming 阶段键盘处理：导航 + 提交/取消/重答。
    fn update_ask_user_confirming_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        let snapshot = self.model.conversation.ask_user_snapshot();
        let confirm_cursor = snapshot.as_ref().map(|s| s.confirm_cursor).unwrap_or(0);
        let n = self
            .input
            .ask_user_state
            .as_ref()
            .map(|s| s.items.len())
            .unwrap_or(0);

        match key.code {
            KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                let next = if confirm_cursor == 0 {
                    n + 1
                } else {
                    confirm_cursor - 1
                };
                self.model
                    .conversation
                    .apply(ConversationIntent::SetAskUserConfirmCursor { cursor: next });
                self.mark_output_dirty();
            }
            KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                let next = (confirm_cursor + 1) % (n + 2);
                self.model
                    .conversation
                    .apply(ConversationIntent::SetAskUserConfirmCursor { cursor: next });
                self.mark_output_dirty();
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                if confirm_cursor < n {
                    // 导航回某题重新作答
                    self.model
                        .conversation
                        .apply(ConversationIntent::NavigateAskUserTo {
                            index: confirm_cursor,
                        });
                    self.mark_output_dirty();
                } else if confirm_cursor == n {
                    // 全部确认提交
                    self.submit_ask_user_batch();
                } else {
                    // 取消
                    self.cancel_ask_user_batch();
                }
            }
            KeyCode::Esc => {
                self.cancel_ask_user_batch();
            }
            _ => {}
        }
        Some(UpdateResult::none())
    }

    /// 提交整个 batch：收集所有答案并回传。
    fn submit_ask_user_batch(&mut self) {
        let state = self.input.ask_user_state.take().unwrap();
        let answers: Vec<String> = state
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                // 从 ConversationModel block 读取答案
                self.model
                    .conversation
                    .blocks
                    .iter()
                    .find_map(|block| {
                        if let crate::tui::model::conversation::block::ConversationBlock::AskUserBatch { slots, .. } = block {
                            slots.get(i).and_then(|slot| slot.answer.clone())
                        } else {
                            None
                        }
                    })
                    .or_else(|| item.default.clone())
                    .unwrap_or_default()
            })
            .collect();
        self.model
            .conversation
            .apply(ConversationIntent::ConfirmAskUserBatch);
        self.mark_output_dirty();
        let _ = state.reply_tx.send(answers);
        self.spinner_phase(SpinnerPhase::Generating);
    }

    /// 取消整个 batch：回传空答案。
    fn cancel_ask_user_batch(&mut self) {
        if let Some(state) = self.input.ask_user_state.take() {
            let empty: Vec<String> = state.items.iter().map(|_| String::new()).collect();
            self.dismiss_ask_user_block();
            self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
            let _ = state.reply_tx.send(empty);
            self.spinner_phase(SpinnerPhase::Generating);
        }
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
                    self.set_ask_user_chat_input(false);
                    self.model
                        .conversation
                        .apply(ConversationIntent::AnswerCurrentAskUser { answer: text });
                    self.mark_output_dirty();
                    self.handle_input_intent(crate::tui::model::input::intent::InputIntent::Clear);
                }
            }
            KeyCode::Esc => {
                self.set_ask_user_chat_input(false);
            }
            KeyCode::Backspace => {
                self.model
                    .conversation
                    .apply(ConversationIntent::DeleteAskUserChatChar);
                self.mark_output_dirty();
            }
            KeyCode::Char(c) => {
                self.model
                    .conversation
                    .apply(ConversationIntent::AppendAskUserChatChar { ch: c });
                self.mark_output_dirty();
            }
            KeyCode::Up => {
                let snapshot = self.model.conversation.ask_user_snapshot();
                if let Some(snap) = snapshot {
                    let last = snap.cursor;
                    if last > 0 {
                        self.set_ask_user_chat_input(false);
                        self.set_ask_user_cursor(last);
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

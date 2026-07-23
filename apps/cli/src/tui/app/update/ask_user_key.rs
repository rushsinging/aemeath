use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::model::conversation::block::AskUserPhase;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::spinner::SpinnerPhase;
use crate::tui::model::output_timeline::OutputTimelineItem;
use crate::tui::update::intent::AgentIntent;
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
                let active_index = snap.active_index;
                // items（self.input）与 active_index（conversation snapshot）是两个真相源，
                // 不同步时越界——防御性返回，避免 panic 退出整个 TUI。
                let Some(active_item) = self
                    .input
                    .ask_user_state
                    .as_ref()
                    .and_then(|s| s.items.get(active_index))
                else {
                    crate::tui::log_warn!("ask_user active_index {} 越界，跳过提交", active_index);
                    return Some(UpdateResult::none());
                };

                let answer = if cursor >= llm_option_count && options_count > 0 {
                    // "Type something..." 被选中 → 切换到自由输入子态
                    self.set_ask_user_chat_input(true);
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
                    return Some(UpdateResult::none());
                };

                let final_answer = if answer.is_empty() {
                    // 使用 default
                    active_item.default.clone().unwrap_or_default()
                } else {
                    answer
                };

                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::AnswerCurrentAskUser(AnswerCurrentAskUser {
                        answer: final_answer,
                    }),
                ));
                self.maybe_auto_submit_ask_user();
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
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SetAskUserConfirmCursor(SetAskUserConfirmCursor {
                        cursor: next,
                    }),
                ));
            }
            KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                let next = (confirm_cursor + 1) % (n + 2);
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::SetAskUserConfirmCursor(SetAskUserConfirmCursor {
                        cursor: next,
                    }),
                ));
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                if confirm_cursor < n {
                    // 导航回某题重新作答
                    self.apply_agent_intent(AgentIntent::Conversation(
                        ConversationIntent::NavigateAskUserTo(NavigateAskUserTo {
                            index: confirm_cursor,
                        }),
                    ));
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

    /// 单问题 Ask 在 answer intent 后已进入确认态，直接提交当前 batch。
    fn maybe_auto_submit_ask_user(&mut self) {
        let has_active_batch = self.model.conversation.ask_user_snapshot().is_some();
        if !has_active_batch {
            self.submit_ask_user_batch();
        }
    }

    /// 提交整个 batch：收集所有答案并回传。
    fn submit_ask_user_batch(&mut self) {
        let state = self.input.ask_user_state.take().unwrap();
        let answers: Vec<String> = state
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                // 从 timeline 读取答案（timeline 是状态真相源）
                self.model
                    .conversation
                    .timeline
                    .items()
                    .iter()
                    .find_map(|tl_item| {
                        if let OutputTimelineItem::AskUserBatch { slots, .. } = tl_item {
                            slots.get(i).and_then(|slot| slot.answer.clone())
                        } else {
                            None
                        }
                    })
                    .or_else(|| item.default.clone())
                    .unwrap_or_default()
            })
            .collect();
        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::ConfirmAskUserBatch(ConfirmAskUserBatch),
        ));
        let _ = state.reply_tx.send(sdk::AskUserReply::Answers(answers));
        self.spinner_phase(SpinnerPhase::Generating);
    }

    /// 取消整个 batch：显式回传取消。
    fn cancel_ask_user_batch(&mut self) {
        if let Some(state) = self.input.ask_user_state.take() {
            self.dismiss_ask_user_block();
            let _ = state.reply_tx.send(sdk::AskUserReply::Cancelled);
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
                    self.apply_agent_intent(AgentIntent::Conversation(
                        ConversationIntent::AnswerCurrentAskUser(AnswerCurrentAskUser {
                            answer: text,
                        }),
                    ));
                    self.maybe_auto_submit_ask_user();
                }
            }
            KeyCode::Esc => {
                self.set_ask_user_chat_input(false);
            }
            // Ctrl+ 修饰键优先匹配
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::DeleteAskUserChatWord(DeleteAskUserChatWord),
                ));
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursorEnd(MoveAskUserChatCursorEnd {
                        to_end: false,
                    }),
                ));
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursorEnd(MoveAskUserChatCursorEnd {
                        to_end: true,
                    }),
                ));
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::DeleteAskUserChatWord(DeleteAskUserChatWord),
                ));
            }
            KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::DeleteAskUserChatChar(DeleteAskUserChatChar),
                ));
            }
            KeyCode::Char(c) if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::AppendAskUserChatChar(AppendAskUserChatChar { ch: c }),
                ));
            }
            KeyCode::Left if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursor(MoveAskUserChatCursor { delta: -1 }),
                ));
            }
            KeyCode::Right if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursor(MoveAskUserChatCursor { delta: 1 }),
                ));
            }
            KeyCode::Home if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursorEnd(MoveAskUserChatCursorEnd {
                        to_end: false,
                    }),
                ));
            }
            KeyCode::End if key.modifiers == KeyModifiers::NONE => {
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::MoveAskUserChatCursorEnd(MoveAskUserChatCursorEnd {
                        to_end: true,
                    }),
                ));
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
}

#[cfg(test)]
mod tests {
    // Enter 提交路径用 `items.get(active_index)` 替代裸索引 `items[active_index]`：
    // active_index（conversation snapshot）与 items（input state）是两个真相源，
    // 越界时 get 返回 None、走 let-else 防御分支，而非数组越界 panic。
    // 此处验证该防御所依赖的 Vec::get 语义。
    #[test]
    fn test_items_get_out_of_bounds_returns_none() {
        let items: Vec<u8> = vec![1, 2, 3];
        assert!(items.get(99).is_none());
        assert!(items.get(3).is_none());
        assert_eq!(items.first(), Some(&1));
    }
}

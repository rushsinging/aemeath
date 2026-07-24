use super::UpdateResult;
use crate::tui::app::App;
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::block::AskUserPhase;
use crate::tui::model::conversation::intent::*;
use crate::tui::update::intent::AgentIntent;
use crossterm::event::{KeyCode, KeyModifiers};
use log;

impl App {
    pub(super) fn update_ask_user_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        // #1384: Ctrl+C must cancel the current run even when an AskUser
        // dialog is active. Fall through to the normal key handler so it
        // emits Effect::CancelCurrentRun or ForceQuit.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            return None;
        }
        // #1384: Esc during processing should also cancel, not just
        // cancel the AskUser dialog.
        if key.modifiers == KeyModifiers::NONE && key.code == KeyCode::Esc && self.chat.is_processing {
            return None;
        }

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

        match phase {
            AskUserPhase::Answering => self.update_ask_user_answering_key(key, &snapshot),
            AskUserPhase::Confirming => self.update_ask_user_confirming_key(key),
        }
    }

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
                if cursor >= llm_option_count && options_count > 0 {
                    // "Type something..." 被选中 → 切换到自由输入子态
                    self.set_ask_user_chat_input(true);
                    return Some(UpdateResult::none());
                }

                // 从 timeline AskUserBatch 的 slots 获取选项全名后回填 answer
                let answer = if multi_select {
                    let chosen: Vec<String> = snap
                        .selected
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| **s)
                        .filter_map(|(i, _)| {
                            self.model
                                .conversation
                                .ask_user_batch_option_text(i)
                        })
                        .collect();
                    if chosen.is_empty() {
                        // 未勾选：回退到 cursor 指定的选项
                        self.model
                            .conversation
                            .ask_user_batch_option_text(cursor)
                            .unwrap_or_default()
                    } else {
                        chosen.join(", ")
                    }
                } else if options_count > 0 {
                    self.model
                        .conversation
                        .ask_user_batch_option_text(cursor)
                        .unwrap_or_default()
                } else {
                    return Some(UpdateResult::none());
                };

                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::AnswerCurrentAskUser(AnswerCurrentAskUser {
                        answer,
                    }),
                ));
                return self.maybe_auto_confirm_ask_user();
            }
            KeyCode::Esc => {
                return self.cancel_ask_user_batch();
            }
            _ => {}
        }
        Some(UpdateResult::none())
    }

    fn update_ask_user_confirming_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        let snapshot = self.model.conversation.ask_user_snapshot();
        let confirm_cursor = snapshot.as_ref().map(|s| s.confirm_cursor).unwrap_or(0);
        let n = self.model.conversation.ask_user_slot_count().unwrap_or(0);

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
                    self.apply_agent_intent(AgentIntent::Conversation(
                        ConversationIntent::NavigateAskUserTo(NavigateAskUserTo {
                            index: confirm_cursor,
                        }),
                    ));
                } else if confirm_cursor == n {
                    return self.confirm_ask_user_batch();
                } else {
                    return self.cancel_ask_user_batch();
                }
            }
            KeyCode::Esc => {
                return self.cancel_ask_user_batch();
            }
            _ => {}
        }
        Some(UpdateResult::none())
    }

    fn maybe_auto_confirm_ask_user(&mut self) -> Option<UpdateResult> {
        let has_active_batch = self.model.conversation.ask_user_snapshot().is_some();
        if !has_active_batch {
            self.confirm_ask_user_batch()
        } else {
            Some(UpdateResult::none())
        }
    }

    fn confirm_ask_user_batch(&mut self) -> Option<UpdateResult> {
        let interaction = self
            .model
            .conversation
            .active_interaction()
            .map(|i| (i.request_id().clone(), i.run_id().clone()));
        log::info!(
            target: crate::LOG_TARGET,
            "[ask_user] confirm_ask_user_batch active_interaction={:?}",
            interaction.as_ref().map(|(id, _)| id.as_str()),
        );
        let Some((request_id, _run_id)) = interaction else {
            self.apply_agent_intent(AgentIntent::Conversation(
                ConversationIntent::ConfirmAskUserBatch(ConfirmAskUserBatch),
            ));
            return Some(UpdateResult::none());
        };

        let answers = self
            .model
            .conversation
            .ask_user_batch_answers()
            .unwrap_or_default();
        log::info!(
            target: crate::LOG_TARGET,
            "[ask_user] confirm answers={:?} request_id={}",
            answers,
            request_id.as_str(),
        );

        self.apply_agent_intent(AgentIntent::Conversation(
            ConversationIntent::ConfirmAskUserBatch(ConfirmAskUserBatch),
        ));

        let reply = crate::tui::model::conversation::interaction::UiInteractionReply::UserAnswers(
            answers,
        );
        Some(UpdateResult {
            effects: vec![Effect::ReplyInteraction {
                request_id,
                reply,
            }],
            spawn_effect: None,
            pending_slash: None,
        })
    }

    fn cancel_ask_user_batch(&mut self) -> Option<UpdateResult> {
        let interaction = self
            .model
            .conversation
            .active_interaction()
            .map(|i| i.request_id().clone());
        self.dismiss_ask_user_block();
        if let Some(request_id) = interaction {
            Some(UpdateResult {
                effects: vec![Effect::CancelInteraction {
                    request_id,
                    reason: crate::tui::model::conversation::interaction::UiInteractionCancelReason::UserCancelled,
                }],
                spawn_effect: None,
                pending_slash: None,
            })
        } else {
            Some(UpdateResult::none())
        }
    }

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
                    self.maybe_auto_confirm_ask_user();
                }
            }
            KeyCode::Esc => {
                self.set_ask_user_chat_input(false);
            }
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
            KeyCode::Char(c)
                if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) =>
            {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.apply_agent_intent(AgentIntent::Conversation(
                    ConversationIntent::AppendAskUserChatChar(AppendAskUserChatChar { ch }),
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

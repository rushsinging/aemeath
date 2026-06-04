use super::agent_progress::AgentProgressEntry;
use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::ids::{ChatId, ToolCallId};
use super::intent::ConversationIntent;
use super::queued_submission::QueuedSubmission;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub blocks: Vec<ConversationBlock>,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,
    next_block_sequence: usize,
    active_text_block_id: Option<String>,
    active_thinking_block_id: Option<String>,
}

impl ConversationModel {
    /// 清空整段对话，回到初始空状态。用于 `/clear` 等需要重置单一真相源的场景。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        match intent {
            ConversationIntent::StartChat { submission } => self.start_chat(submission),
            ConversationIntent::AppendUserMessage { text } => self.append_user_message(text),
            ConversationIntent::ObserveToolCallStart { id, name, index } => {
                self.observe_tool_call_start(id, name, index)
            }
            ConversationIntent::ObserveToolCall {
                id,
                provider_id,
                name,
                index,
                summary,
            } => self.observe_tool_call(id, provider_id, name, index, summary),
            ConversationIntent::ObserveToolResult {
                id,
                provider_id,
                tool_name,
                output,
                is_error,
                image_count,
            } => {
                self.observe_tool_result(id, provider_id, tool_name, output, is_error, image_count)
            }
            ConversationIntent::CompleteChat => self.complete_chat(),
            ConversationIntent::ObserveAssistantText { text } => self.append_assistant_text(text),
            ConversationIntent::ObserveThinkingText { text } => self.append_thinking_text(text),
            ConversationIntent::CompleteTextBlock => self.complete_text_block(),
            ConversationIntent::ObserveToolArguments {
                id,
                name,
                index,
                partial_args,
            } => self.observe_tool_arguments(id, name, index, partial_args),
            ConversationIntent::AppendSystemMessage { text } => self.append_system_message(text),
            ConversationIntent::AppendError { text } => self.append_error(text),
            ConversationIntent::QueueSubmission { text } => self.queue_submission(text),
            ConversationIntent::ClearQueuedSubmissions => self.clear_queued_submissions(),
            ConversationIntent::RecordAgentProgress { tool_id, message } => {
                self.record_agent_progress(tool_id, message)
            }
            ConversationIntent::ShowAskUser {
                question,
                options,
                llm_option_count,
                multi_select,
                cursor,
                default,
            } => self.show_ask_user(
                question,
                options,
                llm_option_count,
                multi_select,
                cursor,
                default,
            ),
            ConversationIntent::SetAskUserCursor { cursor } => self.set_ask_user_cursor(cursor),
            ConversationIntent::ToggleAskUserSelected { index } => {
                self.toggle_ask_user_selected(index)
            }
            ConversationIntent::SetAskUserChatInput { active } => {
                self.set_ask_user_chat_input(active)
            }
            ConversationIntent::AppendAskUserChatChar { ch } => self.append_ask_user_chat_char(ch),
            ConversationIntent::DeleteAskUserChatChar => self.delete_ask_user_chat_char(),
            ConversationIntent::DismissAskUser => self.dismiss_ask_user(),
            ConversationIntent::AnswerAskUser { answer } => self.answer_ask_user(answer),
        }
    }

    fn start_chat(&mut self, submission: String) -> Vec<ConversationChange> {
        self.next_chat_sequence += 1;
        let chat_id = ChatId::new(format!("chat-{}", self.next_chat_sequence));
        let chat = Chat::new(chat_id.clone(), submission.clone());
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        let user_block_id = self.next_block_id("user");
        let turn_id = "turn-1".to_string();
        self.blocks.push(ConversationBlock::UserMessage {
            id: user_block_id,
            text: submission,
        });
        vec![
            ConversationChange::ChatStarted {
                chat_id: chat_id.as_ref().to_string(),
            },
            ConversationChange::ChatTurnStarted {
                chat_id: chat_id.as_ref().to_string(),
                turn_id,
            },
            ConversationChange::OutputDirty,
        ]
    }

    fn append_user_message(&mut self, text: String) -> Vec<ConversationChange> {
        let block_id = self.next_block_id("user");
        self.blocks.push(ConversationBlock::UserMessage {
            id: block_id.clone(),
            text,
        });
        vec![
            ConversationChange::UserMessageAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    fn observe_tool_call_start(
        &mut self,
        id: String,
        name: String,
        index: usize,
    ) -> Vec<ConversationChange> {
        let tool_call_id = ToolCallId::new(id.clone());
        let Some(chat_id) = self.active_chat_id.clone() else {
            return Vec::new();
        };
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                turn.observe_tool_start(tool_call_id.clone(), chat_id, name.clone(), index);
            }
        }
        self.insert_tool_call_block_before_active_text(
            tool_call_id,
            name.clone(),
            String::new(),
            String::new(),
        );
        vec![
            ConversationChange::ToolCallObserved { name, index },
            ConversationChange::OutputDirty,
        ]
    }
    fn observe_tool_call(
        &mut self,
        id: String,
        _provider_id: String,
        name: String,
        index: usize,
        summary: String,
    ) -> Vec<ConversationChange> {
        let mut args_preview = String::new();
        let mut final_summary = summary.clone();
        let mut bound = false;
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(preview) = turn.bind_tool(&id, summary.clone()) {
                    args_preview = preview;
                    if final_summary.is_empty() {
                        if let Some(call) = turn
                            .tool_calls
                            .iter()
                            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id.as_str()))
                        {
                            final_summary = call.summary.clone().unwrap_or_default();
                        }
                    }
                    bound = true;
                }
            }
        }
        if !bound {
            let Some(chat_id) = self.active_chat_id.clone() else {
                return Vec::new();
            };
            if let Some(chat) = self.active_chat_mut() {
                if let Some(turn) = chat.active_turn_mut() {
                    let tool_call_id = ToolCallId::new(id.clone());
                    turn.observe_tool_start(tool_call_id.clone(), chat_id, name.clone(), index);
                    let _ = turn.bind_tool(&id, summary.clone());
                    if final_summary.is_empty() {
                        if let Some(call) = turn
                            .tool_calls
                            .iter()
                            .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id.as_str()))
                        {
                            args_preview = call.args_preview.clone();
                            final_summary = call.summary.clone().unwrap_or_default();
                        }
                    }
                }
            }
        }
        self.promote_orphan_tool_result(&id);
        let existing_tool_position = self.blocks.iter().position(|block| {
            matches!(
                block,
                ConversationBlock::ToolCall { id: block_id, .. } if block_id.as_ref() == id
            )
        });
        if existing_tool_position.is_none() {
            self.insert_tool_call_block_before_active_text(
                ToolCallId::new(id.clone()),
                name.clone(),
                final_summary.clone(),
                args_preview,
            );
        } else {
            for block in &mut self.blocks {
                if let ConversationBlock::ToolCall {
                    id: block_id,
                    summary: block_summary,
                    args_preview: block_args,
                    ..
                } = block
                {
                    if block_id.as_ref() == id {
                        if !final_summary.is_empty() {
                            *block_summary = final_summary.clone();
                        }
                        if !args_preview.is_empty() {
                            *block_args = args_preview.clone();
                        }
                        break;
                    }
                }
            }
        }
        self.move_tool_results_after_tool_call(&id);
        vec![
            ConversationChange::ToolCallBound { id, name },
            ConversationChange::OutputDirty,
        ]
    }
    fn complete_chat(&mut self) -> Vec<ConversationChange> {
        self.active_text_block_id = None;
        self.active_thinking_block_id = None;
        if let Some(chat) = self.active_chat_mut() {
            chat.status = ChatStatus::Completing;
            let chat_id = chat.id.as_ref().to_string();
            return vec![ConversationChange::ChatCompleting { chat_id }];
        }
        Vec::new()
    }

    fn append_assistant_text(&mut self, text: String) -> Vec<ConversationChange> {
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                turn.assistant_stream.push_str(&text);
            }
        }
        self.active_thinking_block_id = None;
        let block_id = self.append_or_extend_text_block(text, false);
        vec![
            ConversationChange::AssistantTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    fn append_thinking_text(&mut self, text: String) -> Vec<ConversationChange> {
        self.active_text_block_id = None;
        let block_id = self.append_or_extend_text_block(text, true);
        vec![
            ConversationChange::ThinkingTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    fn complete_text_block(&mut self) -> Vec<ConversationChange> {
        let block_id = self
            .active_text_block_id
            .take()
            .or_else(|| self.active_thinking_block_id.take());
        vec![
            ConversationChange::TextBlockCompleted { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    fn observe_tool_arguments(
        &mut self,
        id: String,
        name: String,
        index: usize,
        partial_args: String,
    ) -> Vec<ConversationChange> {
        let tool_call_id = ToolCallId::new(id.clone());
        let mut found_call = false;
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(call) = turn
                    .tool_calls
                    .iter_mut()
                    .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id.as_str()))
                {
                    call.update_args(partial_args.clone());
                    found_call = true;
                }
            }
        }
        if !found_call {
            let Some(chat_id) = self.active_chat_id.clone() else {
                return Vec::new();
            };
            if let Some(chat) = self.active_chat_mut() {
                if let Some(turn) = chat.active_turn_mut() {
                    turn.observe_tool_start(tool_call_id.clone(), chat_id, name.clone(), index);
                    if let Some(call) = turn
                        .tool_calls
                        .iter_mut()
                        .find(|call| call.id.as_ref().map(AsRef::as_ref) == Some(id.as_str()))
                    {
                        call.update_args(partial_args.clone());
                    }
                }
            }
            self.insert_tool_call_block_before_active_text(
                tool_call_id.clone(),
                name.clone(),
                String::new(),
                partial_args.clone(),
            );
        }
        for block in &mut self.blocks {
            if let ConversationBlock::ToolCall {
                id: block_id,
                args_preview,
                ..
            } = block
            {
                if block_id.as_ref() == id {
                    *args_preview = partial_args;
                    break;
                }
            }
        }
        vec![ConversationChange::OutputDirty]
    }
    fn queue_submission(&mut self, text: String) -> Vec<ConversationChange> {
        let id = self.next_block_id("queued");
        self.queued_submissions
            .push(QueuedSubmission::new(id.clone(), text.clone()));
        self.blocks.push(ConversationBlock::QueuedUserMessage {
            id: id.clone(),
            text,
        });
        vec![
            ConversationChange::QueuedSubmissionAdded { id },
            ConversationChange::OutputDirty,
        ]
    }

    fn clear_queued_submissions(&mut self) -> Vec<ConversationChange> {
        let count = self.queued_submissions.len();
        self.queued_submissions.clear();
        self.blocks
            .retain(|block| !matches!(block, ConversationBlock::QueuedUserMessage { .. }));
        vec![
            ConversationChange::QueuedSubmissionsCleared { count },
            ConversationChange::OutputDirty,
        ]
    }

    fn record_agent_progress(
        &mut self,
        tool_id: String,
        message: String,
    ) -> Vec<ConversationChange> {
        // 查找匹配的 ToolCall，将进度信息写入其 activities（供 ToolCallBlock 渲染
        // activity_summary），而不是作为独立根级 AgentProgress block 泄露到对话流中。
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(call) = turn
                    .tool_calls
                    .iter_mut()
                    .find(|c| c.id.as_ref().is_some_and(|id| id.as_ref() == tool_id))
                {
                    call.activities.push(message.clone());
                }
            }
        }
        self.agent_progress
            .push(AgentProgressEntry::new(tool_id.clone(), message.clone()));
        vec![ConversationChange::OutputDirty]
    }
    fn append_or_extend_text_block(&mut self, text: String, thinking: bool) -> String {
        let active_id = if thinking {
            self.active_thinking_block_id.clone()
        } else {
            self.active_text_block_id.clone()
        };

        if let Some(block_id) = active_id {
            if let Some(
                ConversationBlock::AssistantText { text: existing, .. }
                | ConversationBlock::Thinking { text: existing, .. },
            ) = self.blocks.iter_mut().find(|block| block.id() == block_id)
            {
                existing.push_str(&text);
                return block_id;
            }
        }

        let prefix = if thinking { "thinking" } else { "assistant" };
        let block_id = self.next_block_id(prefix);
        if thinking {
            self.active_thinking_block_id = Some(block_id.clone());
            self.blocks.push(ConversationBlock::Thinking {
                id: block_id.clone(),
                text,
            });
        } else {
            self.active_text_block_id = Some(block_id.clone());
            self.blocks.push(ConversationBlock::AssistantText {
                id: block_id.clone(),
                text,
            });
        }
        block_id
    }

    pub(super) fn clear_active_text_blocks(&mut self) {
        self.active_text_block_id = None;
        self.active_thinking_block_id = None;
    }

    pub(super) fn next_block_id(&mut self, prefix: &str) -> String {
        self.next_block_sequence += 1;
        format!("{prefix}-{}", self.next_block_sequence)
    }

    pub(super) fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }

    /// 只读访问当前活跃文本块 id（供 tool_order 使用）。
    pub(super) fn active_text_block_id(&self) -> Option<&str> {
        self.active_text_block_id.as_deref()
    }
}

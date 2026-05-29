use super::agent_progress::AgentProgressEntry;
use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::ids::{ChatId, ToolCallId};
use super::intent::ConversationIntent;
use super::queued_submission::QueuedSubmission;
use super::tool_call::ToolCallStatus;

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
            ConversationIntent::ObserveToolCallStart { name, index } => {
                self.observe_tool_call_start(name, index)
            }
            ConversationIntent::ObserveToolCall {
                id,
                name,
                index,
                summary,
            } => self.observe_tool_call(id, name, index, summary),
            ConversationIntent::ObserveToolResult {
                id,
                tool_name,
                output,
                is_error,
                image_count,
            } => self.observe_tool_result(id, tool_name, output, is_error, image_count),
            ConversationIntent::CompleteChat => self.complete_chat(),
            ConversationIntent::ObserveAssistantText { text } => self.append_assistant_text(text),
            ConversationIntent::ObserveThinkingText { text } => self.append_thinking_text(text),
            ConversationIntent::CompleteTextBlock => self.complete_text_block(),
            ConversationIntent::ObserveToolArguments {
                name,
                index,
                partial_args,
            } => self.observe_tool_arguments(name, index, partial_args),
            ConversationIntent::AppendSystemMessage { text } => self.append_system_message(text),
            ConversationIntent::AppendError { text } => self.append_error(text),
            ConversationIntent::QueueSubmission { text } => self.queue_submission(text),
            ConversationIntent::ClearQueuedSubmissions => self.clear_queued_submissions(),
            ConversationIntent::RecordAgentProgress { tool_id, message } => {
                self.record_agent_progress(tool_id, message)
            }
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

    fn observe_tool_call_start(&mut self, name: String, index: usize) -> Vec<ConversationChange> {
        let Some(chat_id) = self.active_chat_id.clone() else {
            return Vec::new();
        };
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                turn.observe_tool_start(chat_id, name.clone(), index);
            }
        }
        vec![
            ConversationChange::ToolCallObserved { name, index },
            ConversationChange::OutputDirty,
        ]
    }

    fn observe_tool_call(
        &mut self,
        id: String,
        name: String,
        index: usize,
        summary: String,
    ) -> Vec<ConversationChange> {
        let Some(chat) = self.active_chat_mut() else {
            return Vec::new();
        };
        let Some(turn) = chat.active_turn_mut() else {
            return Vec::new();
        };
        let Some(args_preview) =
            turn.bind_tool(ToolCallId::new(id.clone()), &name, index, summary.clone())
        else {
            return Vec::new();
        };
        self.blocks.push(ConversationBlock::ToolCall {
            id: ToolCallId::new(id.clone()),
            name: name.clone(),
            summary: summary.clone(),
            args_preview,
        });
        vec![
            ConversationChange::ToolCallBound { id, name },
            ConversationChange::OutputDirty,
        ]
    }

    fn observe_tool_result(
        &mut self,
        id: String,
        _tool_name: String,
        output: String,
        is_error: bool,
        image_count: usize,
    ) -> Vec<ConversationChange> {
        if let Some(status) = self.complete_active_tool(&id, output.clone(), is_error) {
            self.blocks.push(ConversationBlock::ToolResult {
                id: ToolCallId::new(id.clone()),
                output,
                is_error,
                image_count,
            });
            return vec![
                ConversationChange::ToolCallCompleted { id, status },
                ConversationChange::StyleBoundaryResetRequired,
                ConversationChange::OutputDirty,
            ];
        }
        self.blocks.push(ConversationBlock::OrphanToolResult {
            id: id.clone(),
            output,
            is_error,
        });
        vec![
            ConversationChange::OrphanToolResultObserved { id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    fn complete_active_tool(
        &mut self,
        id: &str,
        output: String,
        is_error: bool,
    ) -> Option<ToolCallStatus> {
        let chat = self.active_chat_mut()?;
        let turn = chat.active_turn_mut()?;
        turn.complete_tool(id, output, is_error)
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
        name: String,
        index: usize,
        partial_args: String,
    ) -> Vec<ConversationChange> {
        let mut found_call = false;
        let mut bound_id = None;
        if let Some(chat) = self.active_chat_mut() {
            if let Some(turn) = chat.active_turn_mut() {
                if let Some(call) = turn
                    .tool_calls
                    .iter_mut()
                    .find(|call| call.stream_key.name == name && call.stream_key.index == index)
                {
                    call.update_args(partial_args.clone());
                    found_call = true;
                    bound_id = call.id.clone();
                }
            }
        }
        if let Some(bound_id) = bound_id {
            for block in &mut self.blocks {
                if let ConversationBlock::ToolCall {
                    id, args_preview, ..
                } = block
                {
                    if id == &bound_id {
                        *args_preview = partial_args;
                        break;
                    }
                }
            }
            return vec![ConversationChange::OutputDirty];
        }
        if found_call {
            return vec![ConversationChange::OutputDirty];
        }
        Vec::new()
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
        let block_id = self.next_block_id("agent-progress");
        self.agent_progress
            .push(AgentProgressEntry::new(tool_id.clone(), message.clone()));
        self.blocks.push(ConversationBlock::AgentProgress {
            id: block_id.clone(),
            tool_id: tool_id.clone(),
            message,
        });
        vec![
            ConversationChange::AgentProgressRecorded { block_id, tool_id },
            ConversationChange::OutputDirty,
        ]
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

    fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }
}

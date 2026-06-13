use super::agent_progress::AgentProgressEntry;
use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::intent::ConversationIntent;
use super::queued_submission::QueuedSubmission;
use super::tool_call::ToolCallStatus;
use crate::tui::model::output_timeline::{
    OutputTimelineItem, OutputTimelineModel, TimelineRuntimeContext,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub blocks: Vec<ConversationBlock>,
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,
    next_block_sequence: usize,
    active_text_block_id: Option<String>,
    active_text_context: Option<(ChatId, ChatTurnId)>,
    active_thinking_block_id: Option<String>,
    active_thinking_context: Option<(ChatId, ChatTurnId)>,
}

struct ToolCallUpdateObservation {
    chat_id: ChatId,
    turn_id: ChatTurnId,
    id: ToolCallId,
    provider_id: Option<String>,
    name: String,
    index: usize,
    arguments: Option<String>,
    summary: Option<String>,
    status: ToolCallStatus,
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
            ConversationIntent::ObserveToolCallStart {
                chat_id,
                turn_id,
                id,
                provider_id,
                name,
                index,
            } => self.observe_tool_call_start(chat_id, turn_id, id, provider_id, name, index),
            ConversationIntent::ObserveToolCallUpdate {
                chat_id,
                turn_id,
                id,
                provider_id,
                name,
                index,
                arguments,
                summary,
                status,
            } => self.observe_tool_call_update(ToolCallUpdateObservation {
                chat_id,
                turn_id,
                id,
                provider_id,
                name,
                index,
                arguments,
                summary,
                status,
            }),
            ConversationIntent::ObserveToolResult {
                chat_id,
                turn_id,
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                image_count,
            } => self.observe_tool_result(
                chat_id,
                turn_id,
                id,
                provider_id,
                tool_name,
                output,
                content,
                is_error,
                image_count,
            ),
            ConversationIntent::CompleteChat { chat_id, turn_id } => {
                self.complete_chat(chat_id, turn_id)
            }
            ConversationIntent::ObserveAssistantText {
                chat_id,
                turn_id,
                text,
            } => self.append_assistant_text(chat_id, turn_id, text),
            ConversationIntent::ObserveThinkingText {
                chat_id,
                turn_id,
                text,
            } => self.append_thinking_text(chat_id, turn_id, text),
            ConversationIntent::CompleteBlock { chat_id, turn_id } => {
                self.complete_block(chat_id, turn_id)
            }
            ConversationIntent::AppendSystemMessage { text } => self.append_system_message(text),
            ConversationIntent::AppendHookNotice { content } => self.append_hook_notice(content),
            ConversationIntent::AppendError { text } => self.append_error(text),
            ConversationIntent::QueueSubmission { text } => self.queue_submission(text),
            ConversationIntent::ClearQueuedSubmissions => self.clear_queued_submissions(),
            ConversationIntent::RecordAgentProgress {
                chat_id,
                turn_id,
                tool_id,
                message,
            } => self.record_agent_progress(chat_id, turn_id, tool_id, message),
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
        let chat_id = ChatId::new_v7();
        let chat = Chat::new(chat_id.clone(), submission.clone());
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        let user_block_id = self.next_block_id("user");
        let turn_id = ChatTurnId::new_v7();
        self.blocks.push(ConversationBlock::UserMessage {
            id: user_block_id.clone(),
            text: submission.clone(),
        });
        self.timeline.push(OutputTimelineItem::UserMessage {
            id: user_block_id,
            text: submission,
        });
        vec![
            ConversationChange::ChatStarted {
                chat_id: chat_id.to_string(),
            },
            ConversationChange::ChatTurnStarted {
                chat_id: chat_id.to_string(),
                turn_id: turn_id.to_string(),
            },
            ConversationChange::OutputDirty,
        ]
    }

    fn append_user_message(&mut self, text: String) -> Vec<ConversationChange> {
        let block_id = self.next_block_id("user");
        self.blocks.push(ConversationBlock::UserMessage {
            id: block_id.clone(),
            text: text.clone(),
        });
        self.timeline.push(OutputTimelineItem::UserMessage {
            id: block_id.clone(),
            text,
        });
        vec![
            ConversationChange::UserMessageAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    pub(crate) fn ensure_runtime_turn(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    ) -> (ChatId, ChatTurnId) {
        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
            chat.status = ChatStatus::Running;
            if !chat.turns.iter().any(|turn| turn.id == turn_id) {
                let sequence = chat.turns.len();
                chat.turns.push(ChatTurn::new(turn_id.clone(), sequence));
            }
            return (chat_id, turn_id);
        }
        let mut chat = Chat::new(chat_id.clone(), String::new());
        chat.turns.clear();
        chat.turns.push(ChatTurn::new(turn_id.clone(), 0));
        self.chats.push(chat);
        (chat_id, turn_id)
    }

    fn runtime_turn_mut(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
    ) -> Option<&mut ChatTurn> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.turns.iter_mut().find(|turn| &turn.id == turn_id))
    }

    fn observe_tool_call_start(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        _provider_id: Option<String>,
        name: String,
        index: usize,
    ) -> Vec<ConversationChange> {
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        log::debug!(
            target: "cli::tui::tool_flow",
            "model observe tool_call_start chat_id={} turn_id={} id={} name={} index={} blocks_before={}",
            chat_id,
            turn_id,
            id,
            name,
            index,
            self.blocks.len(),
        );
        let tool_call_id = id.clone();
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            turn.observe_tool_start(tool_call_id.clone(), chat_id.clone(), name.clone(), index);
        }
        self.insert_tool_call_block_before_active_text(chat_id, turn_id, tool_call_id);
        vec![
            ConversationChange::ToolCallObserved { name, index },
            ConversationChange::OutputDirty,
        ]
    }
    fn observe_tool_call_update(
        &mut self,
        update: ToolCallUpdateObservation,
    ) -> Vec<ConversationChange> {
        let ToolCallUpdateObservation {
            chat_id,
            turn_id,
            id,
            provider_id,
            name,
            index,
            arguments,
            summary,
            status,
        } = update;
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        let mut candidate_ids = vec![Some(id.to_string())];
        if let Some(ref pid) = provider_id {
            let pid_as_uuid = ToolCallId::from_legacy_or_new(pid).to_string();
            if !candidate_ids.contains(&Some(pid_as_uuid.clone())) {
                candidate_ids.push(Some(pid_as_uuid));
            }
            candidate_ids.push(Some(pid.clone()));
        }
        let mut bound_id = id.clone();
        let mut args_preview = arguments.clone().unwrap_or_default();
        let mut final_summary = summary.clone().unwrap_or_default();
        let mut bound = false;
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            for candidate_id in candidate_ids.into_iter().flatten() {
                if let Some(preview) =
                    turn.update_tool(&candidate_id, arguments.clone(), summary.clone(), status)
                {
                    args_preview = preview;
                    bound_id = ToolCallId::from_legacy_or_new(&candidate_id);
                    if final_summary.is_empty() {
                        if let Some(call) = turn.tool_calls.iter().find(|call| {
                            call.id.as_ref().map(AsRef::as_ref) == Some(candidate_id.as_str())
                        }) {
                            final_summary = call.summary.clone().unwrap_or_default();
                        }
                    }
                    bound = true;
                    break;
                }
            }
        }
        if !bound {
            if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
                turn.observe_tool_start(id.clone(), chat_id.clone(), name.clone(), index);
                let _ =
                    turn.update_tool(id.as_ref(), arguments.clone(), summary.clone(), status);
                bound_id = id.clone();
                if let Some(call) = turn.tool_calls.iter().find(|call| {
                    call.id.as_ref().map(AsRef::as_ref) == Some(id.as_ref())
                }) {
                    args_preview = call.args_preview.clone();
                    final_summary = call.summary.clone().unwrap_or_default();
                }
            }
        }
        self.promote_orphan_tool_result(&chat_id, &turn_id, bound_id.as_ref());
        let existing_tool_position = self.blocks.iter().position(|block| {
            matches!(
                block,
                ConversationBlock::ToolCall { id: block_id, chat_id: block_chat_id, turn_id: block_turn_id, .. }
                    if block_chat_id == &chat_id && block_turn_id == &turn_id && block_id == &bound_id
            )
        });
        if existing_tool_position.is_none() {
            self.insert_tool_call_block_before_active_text(
                chat_id.clone(),
                turn_id.clone(),
                bound_id.clone(),
            );
        }
        self.move_tool_results_after_tool_call(&chat_id, &turn_id, bound_id.as_ref());
        log::debug!(
            target: "cli::tui::tool_flow",
            "model bound tool_call_update chat_id={} turn_id={} id={} provider_id={:?} bound_id={} name={} index={} status={:?} bound={} args_len={} summary_len={} has_block={} blocks_after={}",
            chat_id,
            turn_id,
            id,
            provider_id,
            bound_id,
            name,
            index,
            status,
            bound,
            args_preview.len(),
            final_summary.len(),
            existing_tool_position.is_some(),
            self.blocks.len(),
        );
        vec![
            ConversationChange::ToolCallBound {
                id: bound_id.to_string(),
                name,
            },
            ConversationChange::OutputDirty,
        ]
    }
    fn complete_chat(&mut self, chat_id: ChatId, turn_id: ChatTurnId) -> Vec<ConversationChange> {
        self.active_text_block_id = None;
        self.active_text_context = None;
        self.active_thinking_block_id = None;
        self.active_thinking_context = None;
        let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) else {
            return Vec::new();
        };
        if !chat.turns.iter().any(|turn| turn.id == turn_id) {
            return Vec::new();
        }
        chat.status = ChatStatus::Completing;
        let chat_id = chat.id.as_ref().to_string();
        vec![ConversationChange::ChatCompleting { chat_id }]
    }

    fn append_assistant_text(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        text: String,
    ) -> Vec<ConversationChange> {
        if text.is_empty() {
            return Vec::new();
        }
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            turn.assistant_stream.push_str(&text);
        }
        self.active_thinking_block_id = None;
        self.active_thinking_context = None;
        let block_id = self.append_or_extend_text_block(chat_id, turn_id, text, false);
        vec![
            ConversationChange::AssistantTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    fn append_thinking_text(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        text: String,
    ) -> Vec<ConversationChange> {
        if text.is_empty() {
            return Vec::new();
        }
        self.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        self.active_text_block_id = None;
        self.active_text_context = None;
        let block_id = self.append_or_extend_text_block(chat_id, turn_id, text, true);
        vec![
            ConversationChange::ThinkingTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    fn complete_block(&mut self, chat_id: ChatId, turn_id: ChatTurnId) -> Vec<ConversationChange> {
        let context = (chat_id, turn_id);
        let block_id = if self.active_text_context.as_ref() == Some(&context) {
            self.active_text_context = None;
            self.active_text_block_id.take()
        } else if self.active_thinking_context.as_ref() == Some(&context) {
            self.active_thinking_context = None;
            self.active_thinking_block_id.take()
        } else {
            None
        };
        vec![
            ConversationChange::BlockCompleted { block_id },
            ConversationChange::StyleBoundaryResetRequired,
            ConversationChange::OutputDirty,
        ]
    }

    fn queue_submission(&mut self, text: String) -> Vec<ConversationChange> {
        let id = self.next_block_id("queued");
        self.queued_submissions
            .push(QueuedSubmission::new(id.clone(), text.clone()));
        self.blocks.push(ConversationBlock::QueuedUserMessage {
            id: id.clone(),
            text: text.clone(),
        });
        self.timeline.push(OutputTimelineItem::QueuedUserMessage {
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
        self.timeline
            .retain(|item| !matches!(item, OutputTimelineItem::QueuedUserMessage { .. }));
        vec![
            ConversationChange::QueuedSubmissionsCleared { count },
            ConversationChange::OutputDirty,
        ]
    }

    fn record_agent_progress(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_id: ToolCallId,
        message: String,
    ) -> Vec<ConversationChange> {
        // 查找匹配的 ToolCall，将进度信息写入其 activities（供 ToolCallBlock 渲染
        // activity_summary），而不是作为独立根级 AgentProgress block 泄露到对话流中。
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &turn_id) {
            if let Some(call) = turn.tool_calls.iter_mut().find(|c| {
                c.id.as_ref()
                    .is_some_and(|id| id.as_ref() == tool_id.to_string())
            }) {
                call.activities.push(message.clone());
            }
        }
        self.agent_progress.push(AgentProgressEntry::new(
            tool_id.to_string(),
            message.clone(),
        ));
        vec![ConversationChange::OutputDirty]
    }
    fn append_or_extend_text_block(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        text: String,
        thinking: bool,
    ) -> String {
        let context = (chat_id.clone(), turn_id.clone());
        let active_id = if thinking {
            (self.active_thinking_context.as_ref() == Some(&context))
                .then(|| self.active_thinking_block_id.clone())
                .flatten()
        } else {
            (self.active_text_context.as_ref() == Some(&context))
                .then(|| self.active_text_block_id.clone())
                .flatten()
        };

        if let Some(block_id) = active_id {
            if let Some(
                ConversationBlock::AssistantText { text: existing, .. }
                | ConversationBlock::Thinking { text: existing, .. },
            ) = self.blocks.iter_mut().find(|block| block.id() == block_id)
            {
                existing.push_str(&text);
                if let Some(
                    OutputTimelineItem::AssistantText { text: existing, .. }
                    | OutputTimelineItem::Thinking { text: existing, .. },
                ) = self
                    .timeline
                    .items_mut()
                    .iter_mut()
                    .find(|item| item.id().as_ref() == block_id)
                {
                    existing.push_str(&text);
                }
                return block_id;
            }
        }

        let prefix = if thinking { "thinking" } else { "assistant" };
        let block_id = self.next_block_id(prefix);
        if thinking {
            self.active_thinking_block_id = Some(block_id.clone());
            self.active_thinking_context = Some(context);
            self.blocks.push(ConversationBlock::Thinking {
                id: block_id.clone(),
                chat_id: Some(chat_id.clone()),
                turn_id: Some(turn_id.clone()),
                text: text.clone(),
            });
            self.timeline.push(OutputTimelineItem::Thinking {
                id: block_id.clone(),
                context: Some(TimelineRuntimeContext::new(chat_id, turn_id)),
                text,
            });
        } else {
            self.active_text_block_id = Some(block_id.clone());
            self.active_text_context = Some(context);
            self.blocks.push(ConversationBlock::AssistantText {
                id: block_id.clone(),
                chat_id: Some(chat_id.clone()),
                turn_id: Some(turn_id.clone()),
                text: text.clone(),
            });
            self.timeline.push(OutputTimelineItem::AssistantText {
                id: block_id.clone(),
                context: Some(TimelineRuntimeContext::new(chat_id, turn_id)),
                text,
            });
        }
        block_id
    }

    pub(super) fn clear_active_text_blocks(&mut self) {
        self.active_text_block_id = None;
        self.active_text_context = None;
        self.active_thinking_block_id = None;
        self.active_thinking_context = None;
    }

    pub(super) fn next_block_id(&mut self, prefix: &str) -> String {
        self.next_block_sequence += 1;
        format!("{prefix}-{}", self.next_block_sequence)
    }

    pub(super) fn active_chat_mut(&mut self) -> Option<&mut Chat> {
        let active = self.active_chat_id.clone()?;
        self.chats.iter_mut().find(|chat| chat.id == active)
    }
}

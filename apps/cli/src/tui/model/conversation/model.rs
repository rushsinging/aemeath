use super::agent_progress::AgentProgressEntry;
use super::change::ConversationChange;
use super::chat::{Chat, ChatStatus};
use super::chat_turn::ChatTurn;
use super::ids::{ChatId, ChatTurnId};
use super::intent::ConversationIntent;
use super::queued_submission::QueuedSubmission;
use super::tool_observe::ToolCallUpdateObservation;
use crate::tui::model::output_timeline::{OutputTimelineItem, OutputTimelineModel};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConversationModel {
    pub chats: Vec<Chat>,
    pub active_chat_id: Option<ChatId>,
    pub timeline: OutputTimelineModel,
    pub queued_submissions: Vec<QueuedSubmission>,
    pub agent_progress: Vec<AgentProgressEntry>,
    next_chat_sequence: usize,
    next_block_sequence: usize,
    /// 单调递增的内容版本号；每次产生 change 的 apply +1。
    /// 供渲染层 memo `assemble_from_conversation`：revision 不变即可复用上次 view_model。
    revision: u64,
    pub(super) active_text_block_id: Option<String>,
    pub(super) active_text_context: Option<(ChatId, ChatTurnId)>,
    pub(super) active_thinking_block_id: Option<String>,
    pub(super) active_thinking_context: Option<(ChatId, ChatTurnId)>,
}

impl ConversationModel {
    /// 清空整段对话，回到初始空状态。用于 `/clear` 等需要重置单一真相源的场景。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn apply(&mut self, intent: ConversationIntent) -> Vec<ConversationChange> {
        let changes = match intent {
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
                status,
            } => self.observe_tool_call_update(ToolCallUpdateObservation {
                chat_id,
                turn_id,
                id,
                provider_id,
                name,
                index,
                arguments,
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
            ConversationIntent::QueueSubmission { input_id, text } => {
                self.queue_submission(input_id, text)
            }
            ConversationIntent::ClearQueuedSubmissionById { input_id } => {
                self.clear_queued_submission_by_id(&input_id)
            }
            ConversationIntent::RecordAgentProgress {
                chat_id,
                turn_id,
                tool_id,
                message,
            } => self.record_agent_progress(chat_id, turn_id, tool_id, message),
            ConversationIntent::ShowAskUserBatch { slots } => self.show_ask_user_batch(slots),
            ConversationIntent::SetAskUserCursor { cursor } => self.set_ask_user_cursor(cursor),
            ConversationIntent::ToggleAskUserSelected { index } => {
                self.toggle_ask_user_selected(index)
            }
            ConversationIntent::SetAskUserChatInput { active } => {
                self.set_ask_user_chat_input(active)
            }
            ConversationIntent::AppendAskUserChatChar { ch } => self.append_ask_user_chat_char(ch),
            ConversationIntent::DeleteAskUserChatChar => self.delete_ask_user_chat_char(),
            ConversationIntent::AnswerCurrentAskUser { answer } => {
                self.answer_current_ask_user(answer)
            }
            ConversationIntent::NavigateAskUserTo { index } => self.navigate_ask_user_to(index),
            ConversationIntent::SetAskUserConfirmCursor { cursor } => {
                self.set_ask_user_confirm_cursor(cursor)
            }
            ConversationIntent::ConfirmAskUserBatch => self.confirm_ask_user_batch(),
            ConversationIntent::DismissAskUserBatch => self.dismiss_ask_user_batch(),
        };
        if !changes.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        changes
    }

    /// 当前内容版本号，供渲染层 memo。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn start_chat(&mut self, submission: String) -> Vec<ConversationChange> {
        self.next_chat_sequence += 1;
        let chat_id = ChatId::new_v7();
        let chat = Chat::new(chat_id.clone(), submission.clone());
        self.active_chat_id = Some(chat_id.clone());
        self.chats.push(chat);
        let user_block_id = self.next_block_id("user");
        let turn_id = ChatTurnId::new_v7();
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

    pub(super) fn runtime_turn_mut(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
    ) -> Option<&mut ChatTurn> {
        self.chats
            .iter_mut()
            .find(|chat| &chat.id == chat_id)
            .and_then(|chat| chat.turns.iter_mut().find(|turn| &turn.id == turn_id))
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

    fn queue_submission(
        &mut self,
        input_id: sdk::InputId,
        text: String,
    ) -> Vec<ConversationChange> {
        let id = self.next_block_id("queued");
        self.queued_submissions.push(QueuedSubmission::new(
            id.clone(),
            input_id.clone(),
            text.clone(),
        ));
        self.timeline.push(OutputTimelineItem::QueuedUserMessage {
            id: id.clone(),
            input_id,
            text,
        });
        vec![
            ConversationChange::QueuedSubmissionAdded { id },
            ConversationChange::OutputDirty,
        ]
    }

    fn clear_queued_submission_by_id(
        &mut self,
        input_id: &sdk::InputId,
    ) -> Vec<ConversationChange> {
        let before = self.queued_submissions.len();
        self.queued_submissions.retain(|q| &q.input_id != input_id);
        self.timeline.retain(|it| {
            !matches!(it,
                OutputTimelineItem::QueuedUserMessage { input_id: tid, .. } if tid == input_id)
        });
        let removed = before - self.queued_submissions.len();
        vec![
            ConversationChange::QueuedSubmissionsCleared { count: removed },
            ConversationChange::OutputDirty,
        ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revision_starts_at_zero() {
        let model = ConversationModel::default();
        assert_eq!(model.revision(), 0, "新建 conversation revision 应为 0");
    }

    #[test]
    fn test_revision_bumps_on_mutating_apply() {
        let mut model = ConversationModel::default();
        let before = model.revision();
        let changes = model.apply(ConversationIntent::AppendUserMessage {
            text: "你好".to_string(),
        });
        assert!(!changes.is_empty(), "AppendUserMessage 应产生 change");
        assert_eq!(
            model.revision(),
            before + 1,
            "产生 change 的 apply 应使 revision +1"
        );
    }

    #[test]
    fn test_revision_unchanged_on_noop_apply() {
        let mut model = ConversationModel::default();
        let before = model.revision();
        // 空文本的 ObserveAssistantText 返回空 change（no-op）。
        let changes = model.apply(ConversationIntent::ObserveAssistantText {
            chat_id: ChatId::new("c1"),
            turn_id: ChatTurnId::new("t1"),
            text: String::new(),
        });
        assert!(changes.is_empty(), "空文本 ObserveAssistantText 应为 no-op");
        assert_eq!(model.revision(), before, "no-op apply 不应改 revision");
    }
}

use super::change::ConversationChange;
use super::ids::{ChatId, ChatTurnId};
use super::model::ConversationModel;
use crate::tui::model::output_timeline::{OutputTimelineItem, TimelineRuntimeContext};

impl ConversationModel {
    pub(super) fn append_assistant_text(
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

    pub(super) fn append_thinking_text(
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

    pub(super) fn complete_block(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
    ) -> Vec<ConversationChange> {
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

    pub(super) fn append_or_extend_text_block(
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
                OutputTimelineItem::AssistantText { text: existing, .. }
                | OutputTimelineItem::Thinking { text: existing, .. },
            ) = self
                .timeline
                .items_mut()
                .iter_mut()
                .find(|item| item.id().as_ref() == block_id)
            {
                existing.push_str(&text);
                return block_id;
            }
        }

        let prefix = if thinking { "thinking" } else { "assistant" };
        let block_id = self.next_block_id(prefix);
        if thinking {
            self.active_thinking_block_id = Some(block_id.clone());
            self.active_thinking_context = Some(context);
            self.timeline.push(OutputTimelineItem::Thinking {
                id: block_id.clone(),
                context: Some(TimelineRuntimeContext::new(chat_id, turn_id)),
                text,
            });
        } else {
            self.active_text_block_id = Some(block_id.clone());
            self.active_text_context = Some(context);
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
}

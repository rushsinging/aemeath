use super::change::ConversationChange;
use super::ids::{ChatId, ChatRunId};
use super::model::ConversationModel;
use crate::tui::model::output_timeline::{OutputTimelineItem, TimelineRuntimeContext};

impl ConversationModel {
    pub(super) fn append_assistant_text(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
        text: String,
    ) -> Vec<ConversationChange> {
        if text.is_empty() {
            return Vec::new();
        }
        self.ensure_runtime_turn(chat_id.clone(), run_id.clone());
        if let Some(turn) = self.runtime_turn_mut(&chat_id, &run_id) {
            turn.assistant_stream.push_str(&text);
        }
        self.active_thinking_block_id = None;
        self.active_thinking_context = None;
        if self.starts_no_visible_block(&chat_id, &run_id, &text, false) {
            return Vec::new();
        }
        let block_id = self.append_or_extend_text_block(chat_id, run_id, text, false);
        vec![
            ConversationChange::AssistantTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn append_thinking_text(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
        text: String,
    ) -> Vec<ConversationChange> {
        if text.is_empty() {
            return Vec::new();
        }
        self.ensure_runtime_turn(chat_id.clone(), run_id.clone());
        self.active_text_block_id = None;
        self.active_text_context = None;
        if self.starts_no_visible_block(&chat_id, &run_id, &text, true) {
            return Vec::new();
        }
        let block_id = self.append_or_extend_text_block(chat_id, run_id, text, true);
        vec![
            ConversationChange::ThinkingTextAppended { block_id },
            ConversationChange::OutputDirty,
        ]
    }

    pub(super) fn complete_block(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
    ) -> Vec<ConversationChange> {
        let context = (chat_id, run_id);
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
        run_id: ChatRunId,
        text: String,
        thinking: bool,
    ) -> String {
        let context = (chat_id.clone(), run_id.clone());
        let active_id = self.active_text_block_id_in(&context, thinking);

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
                context: Some(TimelineRuntimeContext::new(chat_id, run_id)),
                text,
            });
        } else {
            self.active_text_block_id = Some(block_id.clone());
            self.active_text_context = Some(context);
            self.timeline.push(OutputTimelineItem::AssistantText {
                id: block_id.clone(),
                context: Some(TimelineRuntimeContext::new(chat_id, run_id)),
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

    /// 当前上下文里该类型文本的活跃块 id；上下文不匹配（换了 chat/run）视为无活跃块。
    fn active_text_block_id_in(
        &self,
        context: &(ChatId, ChatRunId),
        thinking: bool,
    ) -> Option<String> {
        let (active_context, active_block_id) = if thinking {
            (
                &self.active_thinking_context,
                &self.active_thinking_block_id,
            )
        } else {
            (&self.active_text_context, &self.active_text_block_id)
        };
        if active_context.as_ref() != Some(context) {
            return None;
        }
        active_block_id.clone()
    }

    /// 纯空白 delta 且当前无活跃同类块时不成块：provider（如 Agnes）会在
    /// 思考段结束、工具调用前单独推送 `"\n\n"`，若建块只会渲染出只有
    /// `●` / `💭` marker 的空卡片。已存在活跃块时仍走追加路径，
    /// 保留段内空行分隔语义。
    fn starts_no_visible_block(
        &self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        text: &str,
        thinking: bool,
    ) -> bool {
        let context = (chat_id.clone(), run_id.clone());
        !has_visible_content(text) && self.active_text_block_id_in(&context, thinking).is_none()
    }
}

/// 文本是否含可见内容；纯空白（含全部换行）不构成可见文本块。
pub(crate) fn has_visible_content(text: &str) -> bool {
    !text.trim().is_empty()
}

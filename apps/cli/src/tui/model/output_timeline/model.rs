use super::item::{OutputTimelineItem, TimelineToolCallRef};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputTimelineModel {
    items: Vec<OutputTimelineItem>,
}

impl OutputTimelineModel {
    pub fn items(&self) -> &[OutputTimelineItem] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut Vec<OutputTimelineItem> {
        &mut self.items
    }

    pub fn push(&mut self, item: OutputTimelineItem) {
        self.items.push(item);
    }

    pub fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&OutputTimelineItem) -> bool,
    {
        self.items.retain(|item| keep(item));
    }

    pub fn contains_tool_call(&self, chat_id: &ChatId, turn_id: &ChatTurnId, id: &str) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == *chat_id
                        && reference.context.turn_id == *turn_id
                        && reference.tool_call_id.as_ref() == id
            )
        })
    }

    pub fn contains_tool_result(&self, chat_id: &ChatId, turn_id: &ChatTurnId, id: &str) -> bool {
        self.items.iter().any(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolResult { reference }
                    if reference.context.chat_id == *chat_id
                        && reference.context.turn_id == *turn_id
                        && reference.tool_call_id.as_ref() == id
            )
        })
    }

    pub fn push_tool_call_ref(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_call_id: ToolCallId,
    ) {
        if !self.contains_tool_call(&chat_id, &turn_id, tool_call_id.as_ref()) {
            self.items.push(OutputTimelineItem::ToolCall {
                reference: TimelineToolCallRef::new(chat_id, turn_id, tool_call_id),
            });
        }
    }

    pub fn push_tool_result_ref(
        &mut self,
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_call_id: ToolCallId,
    ) {
        if !self.contains_tool_result(&chat_id, &turn_id, tool_call_id.as_ref()) {
            self.items.push(OutputTimelineItem::ToolResult {
                reference: TimelineToolCallRef::new(chat_id, turn_id, tool_call_id),
            });
        }
    }

    pub fn move_tool_result_after_tool_call(
        &mut self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_call_id: &ToolCallId,
    ) {
        let Some(result_pos) = self.items.iter().position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolResult { reference }
                    if &reference.context.chat_id == chat_id
                        && &reference.context.turn_id == turn_id
                        && &reference.tool_call_id == tool_call_id
            )
        }) else {
            return;
        };
        let result = self.items.remove(result_pos);
        let Some(call_pos) = self.items.iter().position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if &reference.context.chat_id == chat_id
                        && &reference.context.turn_id == turn_id
                        && &reference.tool_call_id == tool_call_id
            )
        }) else {
            self.items.insert(result_pos.min(self.items.len()), result);
            return;
        };
        self.items.insert(call_pos + 1, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> ChatId {
        ChatId::new("chat-1")
    }

    fn turn() -> ChatTurnId {
        ChatTurnId::new("turn-1")
    }

    #[test]
    fn test_push_tool_call_ref_is_idempotent_for_same_context() {
        let mut model = OutputTimelineModel::default();
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1".to_string()));
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1".to_string()));
        assert_eq!(model.items().len(), 1);
    }

    #[test]
    fn test_push_tool_call_ref_allows_same_id_different_turn() {
        let mut model = OutputTimelineModel::default();
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-a"), ToolCallId::new("tool-1".to_string()));
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-b"), ToolCallId::new("tool-1".to_string()));
        assert_eq!(model.items().len(), 2);
    }

    #[test]
    fn test_move_tool_result_after_tool_call_reorders_matching_context_only() {
        let mut model = OutputTimelineModel::default();
        model.push(OutputTimelineItem::ToolResult {
            reference: TimelineToolCallRef::new(chat(), turn(), ToolCallId::new("tool-1".to_string())),
        });
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1".to_string()));
        model.move_tool_result_after_tool_call(&chat(), &turn(), &ToolCallId::new("tool-1".to_string()));
        assert!(matches!(
            model.items()[0],
            OutputTimelineItem::ToolCall { .. }
        ));
        assert!(matches!(
            model.items()[1],
            OutputTimelineItem::ToolResult { .. }
        ));
    }
}

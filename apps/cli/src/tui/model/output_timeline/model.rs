use super::item::{OutputTimelineItem, TimelineToolCallRef};
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::output_view_change::OutputViewChange;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputTimelineModel {
    items: Vec<OutputTimelineItem>,
    pending_view_changes: Vec<OutputViewChange>,
    #[cfg(test)]
    identity_read_count: std::cell::Cell<usize>,
}

impl OutputTimelineModel {
    pub fn items(&self) -> &[OutputTimelineItem] {
        &self.items
    }

    #[cfg(test)]
    pub(crate) fn record_identity_read(&self) {
        self.identity_read_count
            .set(self.identity_read_count.get().saturating_add(1));
    }

    #[cfg(test)]
    pub(crate) fn reset_identity_read_count(&self) {
        self.identity_read_count.set(0);
    }

    #[cfg(test)]
    pub(crate) fn identity_read_count(&self) -> usize {
        self.identity_read_count.get()
    }

    pub fn item(&self, id: &str) -> Option<&OutputTimelineItem> {
        self.items.iter().find(|item| item.id() == id)
    }

    pub fn items_mut(&mut self) -> &mut Vec<OutputTimelineItem> {
        &mut self.items
    }

    pub(crate) fn take_pending_view_changes(&mut self) -> Vec<OutputViewChange> {
        std::mem::take(&mut self.pending_view_changes)
    }

    pub fn push(&mut self, item: OutputTimelineItem) {
        let item_id = item.id().into_owned();
        self.items.push(item);
        self.pending_view_changes
            .push(OutputViewChange::Append { item_id });
    }

    pub fn retain<F>(&mut self, mut keep: F)
    where
        F: FnMut(&OutputTimelineItem) -> bool,
    {
        let mut removed = Vec::new();
        self.items.retain(|item| {
            let keep_item = keep(item);
            if !keep_item {
                removed.push(item.id().into_owned());
            }
            keep_item
        });
        self.pending_view_changes.extend(
            removed
                .into_iter()
                .map(|item_id| OutputViewChange::Remove { item_id }),
        );
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
            self.push(OutputTimelineItem::ToolCall {
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
            self.push(OutputTimelineItem::ToolResult {
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
        let result_id = result.id().into_owned();
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
        self.pending_view_changes.push(OutputViewChange::Remove {
            item_id: result_id.clone(),
        });
        self.pending_view_changes
            .push(OutputViewChange::Append { item_id: result_id });
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
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        assert_eq!(model.items().len(), 1);
    }

    #[test]
    fn test_push_tool_call_ref_allows_same_id_different_turn() {
        let mut model = OutputTimelineModel::default();
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-a"), ToolCallId::new("tool-1"));
        model.push_tool_call_ref(chat(), ChatTurnId::new("turn-b"), ToolCallId::new("tool-1"));
        assert_eq!(model.items().len(), 2);
    }

    #[test]
    fn test_move_tool_result_after_tool_call_reorders_matching_context_only() {
        let mut model = OutputTimelineModel::default();
        model.push(OutputTimelineItem::ToolResult {
            reference: TimelineToolCallRef::new(chat(), turn(), ToolCallId::new("tool-1")),
        });
        model.push_tool_call_ref(chat(), turn(), ToolCallId::new("tool-1"));
        model.move_tool_result_after_tool_call(&chat(), &turn(), &ToolCallId::new("tool-1"));
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

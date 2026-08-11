use std::collections::HashSet;

use super::item::{OutputTimelineItem, TimelineToolCallRef};
use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId};
use crate::tui::model::conversation::output_view_change::OutputViewChange;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputTimelineModel {
    items: Vec<OutputTimelineItem>,
    pending_view_changes: Vec<OutputViewChange>,
    #[cfg(test)]
    identity_read_count: std::cell::Cell<usize>,
    /// ToolCall 存在性索引：push/retain 维护，move 不破坏。
    tool_call_index: HashSet<TimelineToolCallRef>,
    /// ToolResult 存在性索引：push/retain 维护，move 不破坏。
    tool_result_index: HashSet<TimelineToolCallRef>,
    /// OrphanToolResult 存在性索引（key 为 provider tool id）。
    orphan_ids: HashSet<String>,
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
        index_tool_ref(
            &mut self.tool_call_index,
            &mut self.tool_result_index,
            &mut self.orphan_ids,
            &item,
        );
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
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.tool_call_index.clear();
        self.tool_result_index.clear();
        self.orphan_ids.clear();
        for item in &self.items {
            index_tool_ref(
                &mut self.tool_call_index,
                &mut self.tool_result_index,
                &mut self.orphan_ids,
                item,
            );
        }
    }

    pub fn contains_tool_call(&self, chat_id: &ChatId, run_id: &ChatRunId, id: &str) -> bool {
        let reference = TimelineToolCallRef::new(
            chat_id.clone(),
            run_id.clone(),
            ToolCallId::from_legacy_or_new(id),
        );
        self.tool_call_index.contains(&reference)
    }

    pub fn contains_tool_result(&self, chat_id: &ChatId, run_id: &ChatRunId, id: &str) -> bool {
        let reference = TimelineToolCallRef::new(
            chat_id.clone(),
            run_id.clone(),
            ToolCallId::from_legacy_or_new(id),
        );
        self.tool_result_index.contains(&reference)
    }

    /// OrphanToolResult 是否存在（key 为 provider tool id）。
    pub fn contains_orphan(&self, id: &str) -> bool {
        self.orphan_ids.contains(id)
    }

    pub fn push_tool_call_ref(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
        tool_call_id: ToolCallId,
    ) {
        let reference = TimelineToolCallRef::new(chat_id, run_id, tool_call_id);
        if !self.tool_call_index.contains(&reference) {
            self.push(OutputTimelineItem::ToolCall { reference });
        }
    }

    pub fn push_tool_result_ref(
        &mut self,
        chat_id: ChatId,
        run_id: ChatRunId,
        tool_call_id: ToolCallId,
    ) {
        let reference = TimelineToolCallRef::new(chat_id, run_id, tool_call_id);
        if !self.tool_result_index.contains(&reference) {
            self.push(OutputTimelineItem::ToolResult { reference });
        }
    }

    pub fn move_tool_result_after_tool_call(
        &mut self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_call_id: &ToolCallId,
    ) {
        if !self.tool_result_index.contains(&TimelineToolCallRef::new(
            chat_id.clone(),
            run_id.clone(),
            tool_call_id.clone(),
        )) {
            return;
        }
        let Some(result_pos) = self.items.iter().position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolResult { reference }
                    if &reference.context.chat_id == chat_id
                        && &reference.context.run_id == run_id
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
                        && &reference.context.run_id == run_id
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

fn index_tool_ref(
    tool_calls: &mut HashSet<TimelineToolCallRef>,
    tool_results: &mut HashSet<TimelineToolCallRef>,
    orphans: &mut HashSet<String>,
    item: &OutputTimelineItem,
) {
    match item {
        OutputTimelineItem::ToolCall { reference } => {
            tool_calls.insert(reference.clone());
        }
        OutputTimelineItem::ToolResult { reference } => {
            tool_results.insert(reference.clone());
        }
        OutputTimelineItem::OrphanToolResult { id, .. } => {
            orphans.insert(id.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> ChatId {
        ChatId::new("chat-1")
    }

    fn turn() -> ChatRunId {
        ChatRunId::new("turn-1")
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
        model.push_tool_call_ref(chat(), ChatRunId::new("turn-a"), ToolCallId::new("tool-1"));
        model.push_tool_call_ref(chat(), ChatRunId::new("turn-b"), ToolCallId::new("tool-1"));
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

    #[test]
    fn tool_ref_index_stays_consistent_across_push_and_move() {
        let mut model = OutputTimelineModel::default();
        let chat = ChatId::new("chat-1");
        let turn = ChatRunId::new("turn-1");
        let tool = ToolCallId::new("tool-1");

        model.push_tool_call_ref(chat.clone(), turn.clone(), tool.clone());
        assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
        assert!(!model.contains_tool_result(&chat, &turn, tool.as_ref()));

        model.push_tool_result_ref(chat.clone(), turn.clone(), tool.clone());
        assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
        assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));

        // move 只搬移位置不增删，索引必须保持（remove+insert 后仍命中）。
        model.move_tool_result_after_tool_call(&chat, &turn, &tool);
        assert!(model.contains_tool_call(&chat, &turn, tool.as_ref()));
        assert!(model.contains_tool_result(&chat, &turn, tool.as_ref()));
    }

    #[test]
    fn tool_ref_index_rebuilds_after_retain() {
        let mut model = OutputTimelineModel::default();
        let chat = ChatId::new("chat-1");
        let turn = ChatRunId::new("turn-1");
        let keep_tool = ToolCallId::new("tool-keep");
        let drop_tool = ToolCallId::new("tool-drop");

        model.push_tool_call_ref(chat.clone(), turn.clone(), keep_tool.clone());
        model.push_tool_call_ref(chat.clone(), turn.clone(), drop_tool.clone());
        assert!(model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));

        model.retain(|item| {
            !matches!(item, OutputTimelineItem::ToolCall { reference }
                if reference.tool_call_id == drop_tool)
        });

        assert!(model.contains_tool_call(&chat, &turn, keep_tool.as_ref()));
        assert!(!model.contains_tool_call(&chat, &turn, drop_tool.as_ref()));
    }

    #[test]
    fn orphan_ids_index_tracks_pushed_and_retained_items() {
        let mut model = OutputTimelineModel::default();

        model.push(OutputTimelineItem::OrphanToolResult {
            id: "orphan-1".to_string(),
            tool_name: "Bash".to_string(),
            output: "out".to_string(),
            content: serde_json::json!({}),
            is_error: false,
        });
        assert!(model.contains_orphan("orphan-1"));

        model.retain(|item| !matches!(item, OutputTimelineItem::OrphanToolResult { .. }));
        assert!(!model.contains_orphan("orphan-1"));
    }
}

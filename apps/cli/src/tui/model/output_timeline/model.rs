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

    #[cfg(test)]
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
#[path = "model_tests.rs"]
mod tests;

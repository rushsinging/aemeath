use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::output_view_change::{
    OutputViewChange, OutputViewChanges, OutputViewCursor,
};
use crate::tui::model::display_history::DisplayHistoryModel;
use crate::tui::view_assembler::output::OutputViewAssembler;
use crate::tui::view_assembler::output_tool_lookup::ConversationToolLookup;
use crate::tui::view_assembler::output_window_index::{OutputWindowIndex, OutputWindowIndexChange};
use crate::tui::view_model::{BlockNode, OutputRenderWindow};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PLACEHOLDER_ID: &str = "model-stream-placeholder";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RetainedOutputViewStats {
    pub did_rebuild: bool,
    pub rebuilt_roots: usize,
    pub touched_roots: usize,
    pub created_roots: usize,
    pub reused_roots: usize,
}

pub(crate) struct MaterializedOutputWindow {
    pub(crate) view_model: crate::tui::view_model::OutputViewModel,
    pub(crate) stats: RetainedOutputViewStats,
    pub(crate) indexed_items: usize,
    pub(crate) missing_history_request: Option<sdk::DisplayHistoryWindowRequest>,
}

#[derive(Debug, Default)]
pub(crate) struct RetainedOutputView {
    cursor: Option<OutputViewCursor>,
    workspace_root: Option<PathBuf>,
    window_index: OutputWindowIndex,
    current_window: crate::tui::view_model::OutputViewModel,
    display_history_invalidated: bool,
    root_cache: HashMap<String, Arc<BlockNode>>,
}

impl RetainedOutputView {
    #[cfg(test)]
    pub(crate) fn roots(&self) -> &[Arc<BlockNode>] {
        &self.current_window.roots
    }

    #[cfg(test)]
    pub(crate) fn cached_root_count(&self) -> usize {
        self.root_cache.len()
    }

    pub(crate) fn view_model(&self) -> &crate::tui::view_model::OutputViewModel {
        &self.current_window
    }

    pub(crate) fn invalidate_display_history(&mut self) {
        self.display_history_invalidated = true;
    }

    pub(crate) fn materialize_window(
        &mut self,
        conversation: &ConversationModel,
        display_history: &DisplayHistoryModel,
        workspace_root: Option<&Path>,
        request: OutputRenderWindow,
    ) -> MaterializedOutputWindow {
        let requested_workspace = workspace_root.map(Path::to_path_buf);
        let mut stats = RetainedOutputViewStats::default();
        if self.cursor.is_none()
            || self.workspace_root != requested_workspace
            || self.display_history_invalidated
        {
            self.rebuild_index(
                conversation,
                display_history,
                requested_workspace,
                &mut stats,
            );
        } else {
            self.apply_pending_changes(conversation, display_history, workspace_root, &mut stats);
        }

        let selection = self.window_index.select_window(request);
        let requested_item_ids = selection
            .item_range
            .clone()
            .filter_map(|position| self.window_index.entry_id(position))
            .map(str::to_string)
            .collect::<Vec<_>>();
        let visible_item_ids = requested_item_ids.iter().cloned().collect::<HashSet<_>>();
        let missing_history_request = display_history.window_request(&requested_item_ids);
        self.root_cache
            .retain(|item_id, _| visible_item_ids.contains(item_id));
        let lookup = ConversationToolLookup::new(conversation);
        let mut reused_roots = 0;
        let roots = selection
            .item_range
            .clone()
            .filter_map(|position| {
                let item_id = self.window_index.entry_id(position)?;
                if let Some(root) = self.root_cache.get(item_id) {
                    reused_roots += 1;
                    return Some(Arc::clone(root));
                }
                let root = if item_id == PLACEHOLDER_ID {
                    OutputViewAssembler::assemble_placeholder(conversation)
                } else if let Some(item) = display_history.item(item_id) {
                    crate::tui::view_assembler::resumed_history::assemble_resumed_history_item(
                        display_history,
                        item,
                    )
                } else {
                    let item = conversation.timeline.item(item_id)?;
                    OutputViewAssembler::assemble_item(item, &lookup, workspace_root)
                }?;
                let root = Arc::new(root);
                self.root_cache
                    .insert(item_id.to_string(), Arc::clone(&root));
                Some(root)
            })
            .collect::<Vec<_>>();
        stats.touched_roots = roots.len().saturating_sub(reused_roots);
        stats.created_roots = roots.len().saturating_sub(reused_roots);
        stats.reused_roots = reused_roots;
        let view_model = crate::tui::view_model::OutputViewModel {
            roots,
            version: conversation.revision(),
            follow_tail_hint: true,
            source_total_lines: Some(selection.source_total_lines),
            folded_earlier_lines: selection.folded_earlier_lines,
        };
        self.current_window = view_model.clone();
        MaterializedOutputWindow {
            view_model,
            stats,
            indexed_items: self.window_index.len(),
            missing_history_request,
        }
    }

    fn rebuild_index(
        &mut self,
        conversation: &ConversationModel,
        display_history: &DisplayHistoryModel,
        workspace_root: Option<PathBuf>,
        stats: &mut RetainedOutputViewStats,
    ) {
        let mut entries = display_history
            .items()
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    OutputWindowIndex::estimated_lines_for_history_item(item),
                )
            })
            .chain(
                conversation
                    .timeline
                    .items()
                    .iter()
                    .filter(|item| OutputWindowIndex::estimated_lines_for_item(item) > 0)
                    .map(|item| {
                        (
                            item.id().into_owned(),
                            OutputWindowIndex::estimated_lines_for_item(item),
                        )
                    }),
            )
            .collect::<Vec<_>>();
        if conversation.model_stream_placeholder.is_some() {
            entries.push((PLACEHOLDER_ID.to_string(), 1));
        }
        self.window_index
            .apply_change(OutputWindowIndexChange::Reset { entries });
        self.root_cache.clear();
        self.cursor = Some(conversation.output_view_cursor());
        self.workspace_root = workspace_root;
        self.display_history_invalidated = false;
        stats.did_rebuild = true;
        stats.rebuilt_roots = self.window_index.len();
    }

    fn apply_pending_changes(
        &mut self,
        conversation: &ConversationModel,
        display_history: &DisplayHistoryModel,
        workspace_root: Option<&Path>,
        stats: &mut RetainedOutputViewStats,
    ) {
        let cursor = self.cursor.expect("cursor checked above");
        match conversation.output_view_changes_since(cursor) {
            OutputViewChanges::RebuildRequired { next_cursor: _ } => {
                self.rebuild_index(
                    conversation,
                    display_history,
                    workspace_root.map(Path::to_path_buf),
                    stats,
                );
            }
            OutputViewChanges::Delta {
                next_cursor,
                changes,
            } => {
                let mut invalidated_item_ids = HashSet::new();
                for change in changes {
                    match change {
                        OutputViewChange::Append { item_id } => {
                            invalidated_item_ids.insert(item_id.clone());
                            if let Some(item) = conversation.timeline.item(&item_id) {
                                let estimated_lines =
                                    OutputWindowIndex::estimated_lines_for_item(item);
                                if estimated_lines > 0 {
                                    self.window_index.apply_change(
                                        OutputWindowIndexChange::Append {
                                            item_id,
                                            estimated_lines,
                                        },
                                    );
                                }
                            }
                        }
                        OutputViewChange::Update { item_id } => {
                            invalidated_item_ids.insert(item_id.clone());
                            if let Some(item) = conversation.timeline.item(&item_id) {
                                self.window_index
                                    .apply_change(OutputWindowIndexChange::Update {
                                        item_id,
                                        estimated_lines:
                                            OutputWindowIndex::estimated_lines_for_item(item),
                                    });
                            }
                        }
                        OutputViewChange::Remove { item_id } => {
                            invalidated_item_ids.insert(item_id.clone());
                            self.window_index
                                .apply_change(OutputWindowIndexChange::Remove { item_id });
                        }
                        OutputViewChange::Placeholder => {
                            invalidated_item_ids.insert(PLACEHOLDER_ID.to_string());
                            let change = if conversation.model_stream_placeholder.is_some() {
                                OutputWindowIndexChange::Append {
                                    item_id: PLACEHOLDER_ID.to_string(),
                                    estimated_lines: 1,
                                }
                            } else {
                                OutputWindowIndexChange::Remove {
                                    item_id: PLACEHOLDER_ID.to_string(),
                                }
                            };
                            self.window_index.apply_change(change);
                        }
                        OutputViewChange::Reset => {
                            self.rebuild_index(
                                conversation,
                                display_history,
                                workspace_root.map(Path::to_path_buf),
                                stats,
                            );
                            return;
                        }
                    }
                }
                for item_id in invalidated_item_ids {
                    self.root_cache.remove(&item_id);
                }
                self.cursor = Some(next_cursor);
            }
        }
    }
}

#[cfg(test)]
#[path = "retained_output_view_tests.rs"]
mod tests;

use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_assembler::output::OutputViewAssembler;
use crate::tui::view_assembler::output_tool_lookup::ConversationToolLookup;
#[cfg(test)]
use crate::tui::view_model::BlockNode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RetainedOutputViewStats {
    pub did_rebuild: bool,
    pub rebuilt_roots: usize,
    pub touched_roots: usize,
    pub created_roots: usize,
    pub reused_roots: usize,
}

#[derive(Debug, Default)]
pub(crate) struct RetainedOutputView {
    cursor: Option<crate::tui::model::conversation::output_view_change::OutputViewCursor>,
    workspace_root: Option<PathBuf>,
    view_model: crate::tui::view_model::OutputViewModel,
    root_item_ids: Vec<String>,
    positions: HashMap<String, usize>,
}

impl RetainedOutputView {
    #[cfg(test)]
    pub(crate) fn roots(&self) -> &[Arc<BlockNode>] {
        &self.view_model.roots
    }

    pub(crate) fn view_model(&self) -> &crate::tui::view_model::OutputViewModel {
        &self.view_model
    }

    pub(crate) fn sync(
        &mut self,
        conversation: &ConversationModel,
        workspace_root: Option<&Path>,
    ) -> RetainedOutputViewStats {
        let requested_workspace = workspace_root.map(Path::to_path_buf);
        if self.cursor.is_none() || self.workspace_root != requested_workspace {
            return self.rebuild(conversation, requested_workspace);
        }

        let cursor = self.cursor.expect("cursor checked above");
        match conversation.output_view_changes_since(cursor) {
            crate::tui::model::conversation::output_view_change::OutputViewChanges::RebuildRequired {
                ..
            } => self.rebuild(conversation, requested_workspace),
            crate::tui::model::conversation::output_view_change::OutputViewChanges::Delta {
                next_cursor,
                changes,
            } => {
                let mut stats = RetainedOutputViewStats::default();
                for change in changes {
                    use crate::tui::model::conversation::output_view_change::OutputViewChange;
                    match change {
                        OutputViewChange::Append { item_id } => {
                            self.append(conversation, &item_id, workspace_root, &mut stats);
                        }
                        OutputViewChange::Update { item_id } => {
                            self.update(conversation, &item_id, workspace_root, &mut stats);
                        }
                        OutputViewChange::Remove { item_id } => {
                            self.remove(&item_id, &mut stats);
                        }
                        OutputViewChange::Placeholder => {
                            self.update_placeholder(conversation, &mut stats);
                        }
                        OutputViewChange::Reset => {
                            return self.rebuild(conversation, requested_workspace);
                        }
                    }
                }
                self.cursor = Some(next_cursor);
                self.view_model.version = conversation.revision();
                stats.reused_roots = self
                    .view_model
                    .roots
                    .len()
                    .saturating_sub(stats.created_roots);
                stats
            }
        }
    }

    fn rebuild(
        &mut self,
        conversation: &ConversationModel,
        workspace_root: Option<PathBuf>,
    ) -> RetainedOutputViewStats {
        self.view_model = crate::tui::view_model::OutputViewModel {
            roots: OutputViewAssembler::assemble_shared_roots(
                conversation,
                workspace_root.as_deref(),
            ),
            version: conversation.revision(),
            follow_tail_hint: true,
        };
        self.workspace_root = workspace_root;
        self.cursor = Some(conversation.output_view_cursor());
        self.root_item_ids = retained_item_ids(conversation);
        self.reindex();
        RetainedOutputViewStats {
            did_rebuild: true,
            rebuilt_roots: self.view_model.roots.len(),
            touched_roots: self.view_model.roots.len(),
            created_roots: self.view_model.roots.len(),
            reused_roots: 0,
        }
    }

    fn append(
        &mut self,
        conversation: &ConversationModel,
        item_id: &str,
        workspace_root: Option<&Path>,
        stats: &mut RetainedOutputViewStats,
    ) {
        let Some(item) = conversation.timeline.item(item_id) else {
            return;
        };
        let lookup = ConversationToolLookup::new(conversation);
        if let Some(root) = OutputViewAssembler::assemble_item(item, &lookup, workspace_root) {
            let position = self.view_model.roots.len();
            self.positions.insert(item_id.to_string(), position);
            self.root_item_ids.push(item_id.to_string());
            self.view_model.roots.push(Arc::new(root));
            stats.touched_roots += 1;
            stats.created_roots += 1;
        }
    }

    fn update(
        &mut self,
        conversation: &ConversationModel,
        item_id: &str,
        workspace_root: Option<&Path>,
        stats: &mut RetainedOutputViewStats,
    ) {
        let Some(position) = self.positions.get(item_id).copied() else {
            self.append(conversation, item_id, workspace_root, stats);
            return;
        };
        let Some(item) = conversation.timeline.item(item_id) else {
            self.remove(item_id, stats);
            return;
        };
        let lookup = ConversationToolLookup::new(conversation);
        match OutputViewAssembler::assemble_item(item, &lookup, workspace_root) {
            Some(root) => {
                self.view_model.roots[position] = Arc::new(root);
                stats.touched_roots += 1;
                stats.created_roots += 1;
            }
            None => self.remove(item_id, stats),
        }
    }

    fn remove(&mut self, item_id: &str, stats: &mut RetainedOutputViewStats) {
        let Some(position) = self.positions.remove(item_id) else {
            return;
        };
        self.view_model.roots.remove(position);
        self.root_item_ids.remove(position);
        self.reindex();
        stats.touched_roots += 1;
    }

    fn update_placeholder(
        &mut self,
        conversation: &ConversationModel,
        stats: &mut RetainedOutputViewStats,
    ) {
        const PLACEHOLDER_ID: &str = "model-stream-placeholder";
        match (
            self.positions.get(PLACEHOLDER_ID).copied(),
            OutputViewAssembler::assemble_placeholder(conversation),
        ) {
            (Some(position), Some(root)) => {
                self.view_model.roots[position] = Arc::new(root);
                stats.touched_roots += 1;
                stats.created_roots += 1;
            }
            (None, Some(root)) => {
                self.positions
                    .insert(PLACEHOLDER_ID.to_string(), self.view_model.roots.len());
                self.root_item_ids.push(PLACEHOLDER_ID.to_string());
                self.view_model.roots.push(Arc::new(root));
                stats.touched_roots += 1;
                stats.created_roots += 1;
            }
            (Some(_), None) => self.remove(PLACEHOLDER_ID, stats),
            (None, None) => {}
        }
    }

    fn reindex(&mut self) {
        self.positions = self
            .root_item_ids
            .iter()
            .enumerate()
            .map(|(position, item_id)| (item_id.clone(), position))
            .collect();
    }
}

fn retained_item_ids(conversation: &ConversationModel) -> Vec<String> {
    let lookup = ConversationToolLookup::new(conversation);
    let mut item_ids = conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| {
            OutputViewAssembler::assemble_item(item, &lookup, None).map(|_| item.id().into_owned())
        })
        .collect::<Vec<_>>();
    if conversation.model_stream_placeholder.is_some() {
        item_ids.push("model-stream-placeholder".to_string());
    }
    item_ids
}

#[cfg(test)]
#[path = "retained_output_view_tests.rs"]
mod tests;

use std::collections::HashMap;
use std::ops::Range;

use crate::tui::view_model::OutputRenderWindow;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutputWindowSelection {
    pub(crate) item_range: Range<usize>,
    pub(crate) source_total_lines: usize,
    pub(crate) folded_earlier_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExactRootLayout {
    width: u16,
    line_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutputWindowEntry {
    pub(crate) item_id: String,
    pub(crate) estimated_lines: usize,
    exact_layout: Option<ExactRootLayout>,
}

impl OutputWindowEntry {
    #[cfg(test)]
    pub(crate) fn exact_lines_for_width(&self, width: u16) -> Option<usize> {
        self.exact_layout
            .as_ref()
            .filter(|layout| layout.width == width)
            .map(|layout| layout.line_count)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputWindowIndexChange {
    Append {
        item_id: String,
        estimated_lines: usize,
    },
    Update {
        item_id: String,
        estimated_lines: usize,
    },
    Remove {
        item_id: String,
    },
    Reset {
        entries: Vec<(String, usize)>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct OutputWindowIndex {
    entries: Vec<OutputWindowEntry>,
    positions: HashMap<String, usize>,
    prefix_lines: Vec<usize>,
    #[cfg(test)]
    selection_entry_reads: std::cell::Cell<usize>,
}

impl OutputWindowIndex {
    #[cfg(test)]
    pub(crate) fn entries(&self) -> &[OutputWindowEntry] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn reset_selection_entry_reads(&self) {
        self.selection_entry_reads.set(0);
    }

    #[cfg(test)]
    pub(crate) fn selection_entry_reads(&self) -> usize {
        self.selection_entry_reads.get()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn apply_change(&mut self, change: OutputWindowIndexChange) {
        match change {
            OutputWindowIndexChange::Append {
                item_id,
                estimated_lines,
            } => {
                if self.positions.contains_key(&item_id) {
                    self.update(item_id, estimated_lines);
                    return;
                }
                self.positions.insert(item_id.clone(), self.entries.len());
                let prefix_total = self.prefix_lines.last().copied().unwrap_or(0);
                self.prefix_lines
                    .push(prefix_total.saturating_add(estimated_lines));
                self.entries.push(OutputWindowEntry {
                    item_id,
                    estimated_lines,
                    exact_layout: None,
                });
            }
            OutputWindowIndexChange::Update {
                item_id,
                estimated_lines,
            } => self.update(item_id, estimated_lines),
            OutputWindowIndexChange::Remove { item_id } => {
                let Some(position) = self.positions.remove(&item_id) else {
                    return;
                };
                self.entries.remove(position);
                self.reindex_from(position);
                self.rebuild_prefix_from(position);
            }
            OutputWindowIndexChange::Reset { entries } => {
                self.entries = entries
                    .into_iter()
                    .map(|(item_id, estimated_lines)| OutputWindowEntry {
                        item_id,
                        estimated_lines,
                        exact_layout: None,
                    })
                    .collect();
                self.reindex_from(0);
                self.rebuild_prefix_from(0);
            }
        }
    }

    pub(crate) fn entry_id(&self, position: usize) -> Option<&str> {
        self.entries
            .get(position)
            .map(|entry| entry.item_id.as_str())
    }

    pub(crate) fn estimated_lines_for_item(
        item: &crate::tui::model::output_timeline::OutputTimelineItem,
    ) -> usize {
        use crate::tui::model::output_timeline::OutputTimelineItem;
        match item {
            OutputTimelineItem::QueuedUserMessage { .. } => 0,
            OutputTimelineItem::UserMessage { text, .. } => {
                text.lines().count().max(1).saturating_add(2)
            }
            OutputTimelineItem::AssistantText { text, .. }
            | OutputTimelineItem::Thinking { text, .. }
            | OutputTimelineItem::StopHookFeedback { text, .. }
            | OutputTimelineItem::System { text, .. }
            | OutputTimelineItem::Error { text, .. } => {
                text.lines().count().max(1).saturating_add(1)
            }
            OutputTimelineItem::AskUserBatch { slots, .. } => slots.len().max(1).saturating_add(1),
            OutputTimelineItem::OrphanToolResult { output, .. } => {
                output.lines().count().max(1).saturating_add(1)
            }
            OutputTimelineItem::ToolCall { .. }
            | OutputTimelineItem::ToolResult { .. }
            | OutputTimelineItem::AgentProgress { .. } => 10,
        }
    }

    pub(crate) fn estimated_lines_for_history_item(
        item: &crate::tui::model::conversation::resumed_history::ResumedHistoryItem,
    ) -> usize {
        item.estimated_lines
    }
    #[cfg(test)]
    pub(crate) fn record_exact_lines(&mut self, item_id: &str, width: u16, line_count: usize) {
        let Some(position) = self.positions.get(item_id).copied() else {
            return;
        };
        self.entries[position].exact_layout = Some(ExactRootLayout { width, line_count });
    }

    pub(crate) fn select_window(&self, request: OutputRenderWindow) -> OutputWindowSelection {
        self.select_window_from_prefix(request, &self.prefix_lines)
    }
    #[cfg(test)]
    pub(crate) fn select_window_for_width(
        &self,
        request: OutputRenderWindow,
        width: u16,
    ) -> OutputWindowSelection {
        let counts = self.entries.iter().map(|entry| {
            entry
                .exact_lines_for_width(width)
                .unwrap_or(entry.estimated_lines)
        });
        self.select_window_with_counts(request, counts)
    }

    #[cfg(test)]
    fn select_window_with_counts(
        &self,
        request: OutputRenderWindow,
        counts: impl Iterator<Item = usize>,
    ) -> OutputWindowSelection {
        let counts = counts.collect::<Vec<_>>();
        let source_total_lines = counts
            .iter()
            .fold(0usize, |total, lines| total.saturating_add(*lines));
        if request.line_limit == 0 || counts.is_empty() {
            return OutputWindowSelection {
                item_range: counts.len()..counts.len(),
                source_total_lines,
                folded_earlier_lines: source_total_lines,
            };
        }
        let mut end = counts.len();
        let mut skipped_newer_lines = 0usize;
        while end > 0 && skipped_newer_lines < request.tail_offset {
            end -= 1;
            skipped_newer_lines = skipped_newer_lines.saturating_add(counts[end]);
        }
        let mut start = end;
        let mut selected_lines = 0usize;
        while start > 0 {
            let candidate_lines = counts[start - 1];
            if selected_lines > 0
                && selected_lines.saturating_add(candidate_lines) > request.line_limit
            {
                break;
            }
            start -= 1;
            selected_lines = selected_lines.saturating_add(candidate_lines);
            if selected_lines > request.line_limit {
                break;
            }
        }
        let folded_earlier_lines = counts[..start]
            .iter()
            .fold(0usize, |total, lines| total.saturating_add(*lines));
        OutputWindowSelection {
            item_range: start..end,
            source_total_lines,
            folded_earlier_lines,
        }
    }

    fn select_window_from_prefix(
        &self,
        request: OutputRenderWindow,
        prefix_lines: &[usize],
    ) -> OutputWindowSelection {
        let source_total_lines = prefix_lines.last().copied().unwrap_or(0);
        if request.line_limit == 0 || prefix_lines.is_empty() {
            return OutputWindowSelection {
                item_range: prefix_lines.len()..prefix_lines.len(),
                source_total_lines,
                folded_earlier_lines: source_total_lines,
            };
        }

        let target_end_lines = source_total_lines.saturating_sub(request.tail_offset);
        let end = if request.tail_offset == 0 {
            prefix_lines.len()
        } else {
            first_prefix_at_least(prefix_lines, target_end_lines).saturating_add(1)
        };
        let selected_end_lines = prefix_lines
            .get(end.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let target_start_lines = selected_end_lines.saturating_sub(request.line_limit);
        let start = first_prefix_greater_than(prefix_lines, target_start_lines).min(end);
        #[cfg(test)]
        self.selection_entry_reads.set(
            self.selection_entry_reads
                .get()
                .saturating_add(binary_search_read_bound(prefix_lines.len()).saturating_mul(2)),
        );
        let folded_earlier_lines = prefix_lines
            .get(start.saturating_sub(1))
            .copied()
            .filter(|_| start > 0)
            .unwrap_or(0);
        OutputWindowSelection {
            item_range: start..end,
            source_total_lines,
            folded_earlier_lines,
        }
    }
    #[cfg(test)]
    pub(crate) fn retained_block_nodes(&self) -> usize {
        0
    }

    fn update(&mut self, item_id: String, estimated_lines: usize) {
        let Some(position) = self.positions.get(&item_id).copied() else {
            self.apply_change(OutputWindowIndexChange::Append {
                item_id,
                estimated_lines,
            });
            return;
        };
        let entry = &mut self.entries[position];
        entry.estimated_lines = estimated_lines;
        entry.exact_layout = None;
        self.rebuild_prefix_from(position);
    }

    fn rebuild_prefix_from(&mut self, start: usize) {
        self.prefix_lines.truncate(start);
        let mut total = self.prefix_lines.last().copied().unwrap_or(0);
        for entry in self.entries.iter().skip(start) {
            total = total.saturating_add(entry.estimated_lines);
            self.prefix_lines.push(total);
        }
    }

    fn reindex_from(&mut self, start: usize) {
        if start == 0 {
            self.positions.clear();
        }
        for (position, entry) in self.entries.iter().enumerate().skip(start) {
            self.positions.insert(entry.item_id.clone(), position);
        }
    }
}

fn first_prefix_at_least(prefix_lines: &[usize], target: usize) -> usize {
    prefix_lines.partition_point(|prefix| *prefix < target)
}

fn first_prefix_greater_than(prefix_lines: &[usize], target: usize) -> usize {
    prefix_lines.partition_point(|prefix| *prefix <= target)
}

#[cfg(test)]
fn binary_search_read_bound(entry_count: usize) -> usize {
    (usize::BITS as usize - entry_count.max(1).leading_zeros() as usize).saturating_add(1)
}

#[cfg(test)]
#[path = "output_window_index_tests.rs"]
mod tests;

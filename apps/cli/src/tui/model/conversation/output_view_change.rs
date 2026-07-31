use std::collections::VecDeque;

pub(super) const OUTPUT_VIEW_JOURNAL_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OutputViewCursor(pub(super) u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputViewChange {
    Append { item_id: String },
    Update { item_id: String },
    Remove { item_id: String },
    Reset,
    Placeholder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SequencedOutputViewChange {
    sequence: u64,
    change: OutputViewChange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputViewChanges {
    Delta {
        next_cursor: OutputViewCursor,
        changes: Vec<OutputViewChange>,
    },
    RebuildRequired {
        next_cursor: OutputViewCursor,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct OutputViewJournal {
    sequence: u64,
    entries: VecDeque<SequencedOutputViewChange>,
}

impl OutputViewJournal {
    pub(super) fn cursor(&self) -> OutputViewCursor {
        OutputViewCursor(self.sequence)
    }

    pub(super) fn publish(&mut self, change: OutputViewChange) {
        self.sequence = self.sequence.wrapping_add(1);
        self.entries.push_back(SequencedOutputViewChange {
            sequence: self.sequence,
            change,
        });
        while self.entries.len() > OUTPUT_VIEW_JOURNAL_CAPACITY {
            self.entries.pop_front();
        }
    }

    pub(super) fn changes_since(&self, cursor: OutputViewCursor) -> OutputViewChanges {
        let next_cursor = self.cursor();
        if cursor.0 > self.sequence {
            return OutputViewChanges::RebuildRequired { next_cursor };
        }
        let oldest_available = self
            .entries
            .front()
            .map_or(self.sequence, |entry| entry.sequence.saturating_sub(1));
        if cursor.0 < oldest_available {
            return OutputViewChanges::RebuildRequired { next_cursor };
        }
        OutputViewChanges::Delta {
            next_cursor,
            changes: self
                .entries
                .iter()
                .filter(|entry| entry.sequence > cursor.0)
                .map(|entry| entry.change.clone())
                .collect(),
        }
    }
}

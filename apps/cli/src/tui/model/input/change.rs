use super::completion_item::CompletionItem;
use super::mode::InputMode;
use super::submission::InputSubmission;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputChange {
    TextChanged {
        text: String,
        cursor: usize,
    },
    CursorMoved {
        cursor: usize,
    },
    CompletionChanged {
        visible: bool,
        selected_index: Option<usize>,
        items: Vec<CompletionItem>,
    },
    HistorySelected {
        text: String,
        cursor: usize,
    },
    ModeChanged {
        mode: InputMode,
    },
    Submitted {
        submission: InputSubmission,
    },
    Cleared,
}

#[cfg(test)]
pub fn submitted_text_from_changes(changes: &[InputChange]) -> Option<String> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.text.clone()),
        InputChange::TextChanged { .. }
        | InputChange::CursorMoved { .. }
        | InputChange::CompletionChanged { .. }
        | InputChange::HistorySelected { .. }
        | InputChange::ModeChanged { .. }
        | InputChange::Cleared => None,
    })
}

pub fn submitted_submission_from_changes(changes: &[InputChange]) -> Option<InputSubmission> {
    changes.iter().find_map(|change| match change {
        InputChange::Submitted { submission } => Some(submission.clone()),
        _ => None,
    })
}

#[cfg(test)]
#[path = "change_tests.rs"]
mod tests;

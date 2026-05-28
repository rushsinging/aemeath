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
    },
    AttachmentChanged {
        count: usize,
    },
    ModeChanged {
        mode: InputMode,
    },
    Submitted {
        submission: InputSubmission,
    },
    Cleared,
}

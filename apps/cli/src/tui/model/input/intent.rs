use super::attachment::InputAttachment;
use super::completion_item::CompletionItem;
use super::mode::InputMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertChar(char),
    InsertText(String),
    MoveCursor(usize),
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    DeleteBackward,
    DeleteForward,
    MoveHistoryPrevious,
    MoveHistoryNext,
    SetCompletions {
        query: String,
        items: Vec<CompletionItem>,
    },
    SelectCompletionNext,
    SelectCompletionPrevious,
    AcceptCompletion,
    AttachImage(InputAttachment),
    ClearAttachments,
    SetMode(InputMode),
    Submit,
    Clear,
}

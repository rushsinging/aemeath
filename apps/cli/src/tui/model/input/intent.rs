use super::completion_item::CompletionItem;
use super::mode::InputMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertChar(char),
    InsertText(String),
    ReplaceText(String),
    MoveCursor(usize),
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    InsertNewline,
    DeleteBackward,
    DeleteWordBeforeCursor,
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
    AcceptCompletionValue(String),
    SetAttachmentCount(usize),
    SetMode(InputMode),
    Submit,
    Clear,
}

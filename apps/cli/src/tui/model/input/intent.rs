use super::completion_item::CompletionItem;
use super::mode::InputMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InputIntent {
    InsertChar(char),
    InsertText(String),
    InsertPastedText(String),
    InsertImage(sdk::ClipboardImageView),
    ReplaceText(String),
    MoveCursor(usize),
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorHome,
    MoveCursorEnd,
    InsertNewline,
    DeleteBackward,
    DeleteWordBeforeCursor,
    DeleteForward,
    MoveHistoryPrevious,
    MoveHistoryNext,
    ReplaceHistory(Vec<String>),
    SetCompletions {
        query: String,
        items: Vec<CompletionItem>,
    },
    SelectCompletionNext,
    SelectCompletionPrevious,
    AcceptCompletion,
    AcceptCompletionValue(String),
    SetMode(InputMode),
    Submit,
    Clear,
}
